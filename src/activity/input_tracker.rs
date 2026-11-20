use std::time::{Duration, Instant};

use super::{InputHookConfig, InputHookEvents};

#[derive(Debug, Clone, Copy, Default)]
pub struct InputActivityTracker {
    keyboard: Option<Instant>,
    mouse: Option<Instant>,
}

impl InputActivityTracker {
    pub fn configure(&mut self, config: InputHookConfig, now: Instant) {
        self.keyboard = config.keyboard.then_some(now);
        self.mouse = config.mouse.then_some(now);
    }

    pub fn record(&mut self, events: InputHookEvents, now: Instant) {
        if events.keyboard && self.keyboard.is_some() {
            self.keyboard = Some(now);
        }
        if events.mouse && self.mouse.is_some() {
            self.mouse = Some(now);
        }
    }

    pub fn idle_for(
        &self,
        selected: &crate::config::InputDetectionSettings,
        controller_idle_for: Option<Duration>,
        now: Instant,
    ) -> Option<Duration> {
        let sources = [
            (
                selected.keyboard,
                self.keyboard
                    .map(|last| now.saturating_duration_since(last)),
            ),
            (
                selected.mouse,
                self.mouse.map(|last| now.saturating_duration_since(last)),
            ),
            (selected.controller, controller_idle_for),
        ];
        let mut idle_for = None;
        for (_, elapsed) in sources.into_iter().filter(|(enabled, _)| *enabled) {
            // An unobserved selected source must not be treated as idle.
            let elapsed = elapsed?;
            idle_for = Some(idle_for.map_or(elapsed, |previous: Duration| previous.min(elapsed)));
        }
        idle_for
    }
}
