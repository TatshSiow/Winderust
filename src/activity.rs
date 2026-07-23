use std::time::Duration;

pub mod controller;
pub mod idle_detector;
pub mod input_hook;
pub mod input_tracker;

pub use controller::{ControllerActivityDetector, CONTROLLER_ACTIVITY_POLL_INTERVAL};
pub use idle_detector::{ActivitySnapshot, ActivityState, IdleDetector};
pub use input_hook::{InputHook, InputHookConfig, InputHookEvents};

pub fn merge_activity_snapshot(
    snapshot: ActivitySnapshot,
    additional_idle_for: Option<Duration>,
    idle_timeout: Duration,
) -> ActivitySnapshot {
    let idle_for = min_duration_option(snapshot.idle_for, additional_idle_for);
    match idle_for {
        Some(idle_for) => ActivitySnapshot {
            state: if idle_for >= idle_timeout {
                ActivityState::Idle
            } else {
                ActivityState::Active
            },
            idle_for: Some(idle_for),
        },
        None => snapshot,
    }
}

fn min_duration_option(first: Option<Duration>, second: Option<Duration>) -> Option<Duration> {
    match (first, second) {
        (Some(first), Some(second)) => Some(first.min(second)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_activity_snapshot_uses_recent_controller_activity() {
        let snapshot = ActivitySnapshot {
            state: ActivityState::Idle,
            idle_for: Some(Duration::from_secs(600)),
        };

        let merged = merge_activity_snapshot(
            snapshot,
            Some(Duration::from_secs(2)),
            Duration::from_secs(300),
        );

        assert_eq!(merged.state, ActivityState::Active);
        assert_eq!(merged.idle_for, Some(Duration::from_secs(2)));
    }

    #[test]
    fn merge_activity_snapshot_preserves_unknown_without_additional_input() {
        let snapshot = ActivitySnapshot {
            state: ActivityState::Unknown,
            idle_for: None,
        };

        let merged = merge_activity_snapshot(snapshot, None, Duration::from_secs(300));

        assert_eq!(merged.state, ActivityState::Unknown);
        assert_eq!(merged.idle_for, None);
    }
}
