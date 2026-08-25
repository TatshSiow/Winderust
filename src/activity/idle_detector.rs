use std::time::Duration;

use super::input_tracker;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityState {
    Active,
    Idle,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct ActivitySnapshot {
    pub state: ActivityState,
    pub idle_for: Option<Duration>,
}

pub fn activity_snapshot(idle_timeout: Duration) -> ActivitySnapshot {
    match input_tracker::last_input_elapsed() {
        Some(idle_for) => {
            let state = if idle_for >= idle_timeout {
                ActivityState::Idle
            } else {
                ActivityState::Active
            };
            ActivitySnapshot {
                state,
                idle_for: Some(idle_for),
            }
        }
        None => ActivitySnapshot {
            state: ActivityState::Unknown,
            idle_for: None,
        },
    }
}
