use std::{
    collections::{BTreeMap, VecDeque},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::config::ActionLogMode;

pub const DEFAULT_ACTION_LOG_CAPACITY: usize = 512;
const SKIPPED_ENTRY_DEDUPLICATION_WINDOW_MS: u128 = 30_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionLogEntry {
    pub sequence: u64,
    pub timestamp_epoch_ms: u128,
    pub feature: ActionLogFeature,
    pub process_id: Option<u32>,
    pub process_name: String,
    pub result: ActionLogResult,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ActionLogFeature {
    AppSuspension,
    CpuSetsSoft,
    ProcessorAffinityHard,
    BackgroundEfficiency,
    CoreLimiter,
    ByForeground,
    ByRunningApp,
    ByCpuLoad,
    ByActivity,
    ByTime,
    CpuScheduler,
    ProcessPriority,
    ThreadPriority,
    DynamicPriorityBoost,
    IoPriority,
    GpuPriority,
    MemoryPriority,
    MemoryTrim,
    TimerResolution,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActionLogFeatureSummary {
    pub successful_actions: usize,
    pub failed_actions: usize,
    pub last_success: Option<ActionLogEntry>,
    pub last_failed: Option<ActionLogEntry>,
}

pub type ActionLogSummaries = BTreeMap<ActionLogFeature, ActionLogFeatureSummary>;

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
    revision: u64,
    capacity: usize,
    mode: ActionLogMode,
    summaries: ActionLogSummaries,
}

impl ActionLog {
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            entries: VecDeque::with_capacity(capacity),
            next_sequence: 1,
            revision: 0,
            capacity,
            mode: ActionLogMode::Full,
            summaries: BTreeMap::new(),
        }
    }

    pub fn set_mode(&mut self, mode: ActionLogMode) {
        self.mode = mode;
    }

    pub fn record(
        &mut self,
        feature: ActionLogFeature,
        process_id: Option<u32>,
        process_name: impl Into<String>,
        result: ActionLogResult,
        reason: impl Into<String>,
    ) {
        let process_name = process_name.into();
        let reason = reason.into();
        let timestamp_epoch_ms = timestamp_epoch_ms();
        let should_record = self.mode.should_record(result);
        if result == ActionLogResult::Skipped
            && (!should_record
                || self.entries.iter().rev().any(|entry| {
                    entry.feature == feature
                        && entry.process_id == process_id
                        && entry.process_name == process_name
                        && entry.result == result
                        && entry.reason == reason
                        && timestamp_epoch_ms.saturating_sub(entry.timestamp_epoch_ms)
                            < SKIPPED_ENTRY_DEDUPLICATION_WINDOW_MS
                }))
        {
            return;
        }

        let entry = ActionLogEntry {
            sequence: self.next_sequence,
            timestamp_epoch_ms,
            feature,
            process_id,
            process_name,
            result,
            reason,
        };
        self.next_sequence = self.next_sequence.saturating_add(1);
        match result {
            ActionLogResult::Applied | ActionLogResult::Restored => {
                let summary = self.summaries.entry(feature).or_default();
                summary.successful_actions = summary.successful_actions.saturating_add(1);
                summary.last_success = Some(entry.clone());
            }
            ActionLogResult::Failed => {
                let summary = self.summaries.entry(feature).or_default();
                summary.failed_actions = summary.failed_actions.saturating_add(1);
                summary.last_failed = Some(entry.clone());
            }
            ActionLogResult::Skipped => {}
        }
        if should_record {
            if self.entries.len() == self.capacity {
                self.entries.pop_front();
            }
            self.entries.push_back(entry);
        }
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn entries(&self) -> Vec<ActionLogEntry> {
        self.entries.iter().cloned().collect()
    }

    pub fn summaries(&self) -> ActionLogSummaries {
        self.summaries.clone()
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.summaries.clear();
        self.revision = self.revision.wrapping_add(1);
    }
}

impl ActionLogMode {
    fn should_record(self, result: ActionLogResult) -> bool {
        match self {
            Self::Full => true,
            Self::Warning => {
                matches!(result, ActionLogResult::Failed | ActionLogResult::Skipped)
            }
            Self::Error => matches!(result, ActionLogResult::Failed),
            Self::Off => false,
        }
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
            ActionLogFeature::CoreLimiter,
            Some(1),
            "a.exe",
            ActionLogResult::Applied,
            "first",
        );
        log.record(
            ActionLogFeature::CoreLimiter,
            Some(2),
            "b.exe",
            ActionLogResult::Applied,
            "second",
        );
        log.record(
            ActionLogFeature::CoreLimiter,
            Some(3),
            "c.exe",
            ActionLogResult::Applied,
            "third",
        );

        let entries = log.entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].sequence, 2);
        assert_eq!(entries[1].sequence, 3);
        assert_eq!(entries[1].process_name, "c.exe");
    }

    #[test]
    fn action_log_clear_removes_entries_without_resetting_sequence() {
        let mut log = ActionLog::new(8);
        log.record(
            ActionLogFeature::CoreLimiter,
            Some(1),
            "a.exe",
            ActionLogResult::Applied,
            "first",
        );
        log.clear();
        log.record(
            ActionLogFeature::CoreLimiter,
            Some(2),
            "b.exe",
            ActionLogResult::Applied,
            "second",
        );

        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].sequence, 2);
    }

    #[test]
    fn action_log_mode_filters_records() {
        let mut log = ActionLog::new(8);
        log.set_mode(ActionLogMode::Warning);
        log.record(
            ActionLogFeature::BackgroundEfficiency,
            Some(1),
            "app.exe",
            ActionLogResult::Applied,
            "applied",
        );
        log.record(
            ActionLogFeature::BackgroundEfficiency,
            Some(1),
            "app.exe",
            ActionLogResult::Skipped,
            "skipped",
        );
        log.record(
            ActionLogFeature::BackgroundEfficiency,
            Some(1),
            "app.exe",
            ActionLogResult::Failed,
            "failed",
        );

        let entries = log.entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].result, ActionLogResult::Skipped);
        assert_eq!(entries[1].result, ActionLogResult::Failed);

        let mut error_log = ActionLog::new(8);
        error_log.set_mode(ActionLogMode::Error);
        error_log.record(
            ActionLogFeature::BackgroundEfficiency,
            Some(1),
            "app.exe",
            ActionLogResult::Skipped,
            "skipped",
        );
        error_log.record(
            ActionLogFeature::BackgroundEfficiency,
            Some(1),
            "app.exe",
            ActionLogResult::Failed,
            "failed",
        );

        assert_eq!(error_log.entries().len(), 1);
        assert_eq!(error_log.entries()[0].result, ActionLogResult::Failed);

        log.set_mode(ActionLogMode::Off);
        log.record(
            ActionLogFeature::BackgroundEfficiency,
            Some(2),
            "other.exe",
            ActionLogResult::Restored,
            "restored",
        );

        assert_eq!(log.entries().len(), 2);
        assert_eq!(
            log.summaries()[&ActionLogFeature::BackgroundEfficiency].successful_actions,
            2
        );
    }

    #[test]
    fn summaries_are_not_limited_by_history_capacity_or_visibility_mode() {
        let mut log = ActionLog::new(1);
        log.set_mode(ActionLogMode::Off);
        log.record(
            ActionLogFeature::ByForeground,
            None,
            "",
            ActionLogResult::Applied,
            "first",
        );
        log.record(
            ActionLogFeature::ByForeground,
            None,
            "",
            ActionLogResult::Failed,
            "second",
        );

        assert!(log.entries().is_empty());
        let summary = &log.summaries()[&ActionLogFeature::ByForeground];
        assert_eq!(summary.successful_actions, 1);
        assert_eq!(summary.failed_actions, 1);
        assert_eq!(summary.last_success.as_ref().unwrap().reason, "first");
        assert_eq!(summary.last_failed.as_ref().unwrap().reason, "second");

        log.clear();
        assert!(log.summaries().is_empty());
    }

    #[test]
    fn action_log_coalesces_repeated_skipped_entries() {
        let mut log = ActionLog::new(8);

        for _ in 0..3 {
            log.record(
                ActionLogFeature::ProcessPriority,
                Some(42),
                "service.exe",
                ActionLogResult::Skipped,
                "Access denied.",
            );
        }
        log.record(
            ActionLogFeature::ProcessPriority,
            Some(42),
            "service.exe",
            ActionLogResult::Skipped,
            "Stopped retrying.",
        );

        let entries = log.entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].reason, "Access denied.");
        assert_eq!(entries[1].reason, "Stopped retrying.");
    }
}
