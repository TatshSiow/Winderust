use crate::{
    action_log::{ActionLog, ActionLogFeature, ActionLogResult},
    config::TimerResolutionSettings,
    control::timer_resolution::{
        TimerResolutionController, TimerResolutionInfo, TimerResolutionTransitionStage,
    },
};

const SYSTEM_TARGET_NAME: &str = "System";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TimerResolutionSnapshot {
    pub enabled: bool,
    pub requested_100ns: Option<u32>,
    pub active_rule_process: Option<String>,
    pub maximum_100ns: Option<u32>,
    pub minimum_100ns: Option<u32>,
    pub failed_actions: usize,
    pub message: String,
    pub last_error: Option<String>,
}

#[derive(Default)]
pub struct TimerResolutionManager {
    active_rule_process: Option<String>,
}

impl TimerResolutionManager {
    pub fn update(
        &mut self,
        controller: &mut TimerResolutionController,
        settings: &TimerResolutionSettings,
        automation_enabled: bool,
        foreground_executable_path: Option<&str>,
        action_log: &mut ActionLog,
    ) -> TimerResolutionSnapshot {
        if !automation_enabled {
            return self.disable(controller, action_log, "automation disabled");
        }

        if !settings.enabled {
            return self.disable(controller, action_log, "timer resolution control disabled");
        }

        if !settings.rules.iter().any(|rule| {
            rule.enabled && std::path::Path::new(rule.executable_path.trim()).is_absolute()
        }) {
            return self.release_inactive(
                controller,
                true,
                None,
                action_log,
                "no timer resolution foreground rules are enabled",
                "No timer resolution foreground rules configured.",
            );
        }

        let info = match controller.query() {
            Ok(info) => info,
            Err(err) => {
                let message = err;
                action_log.record(
                    ActionLogFeature::TimerResolution,
                    None,
                    SYSTEM_TARGET_NAME,
                    ActionLogResult::Failed,
                    message.clone(),
                );
                return TimerResolutionSnapshot {
                    enabled: true,
                    failed_actions: 1,
                    message: "Timer resolution query failed.".to_owned(),
                    last_error: Some(message),
                    ..Default::default()
                };
            }
        };

        let Some(foreground_executable_path) =
            foreground_executable_path.filter(|path| !path.trim().is_empty())
        else {
            return self.release_inactive(
                controller,
                true,
                Some(info),
                action_log,
                "foreground app is unknown",
                "Paused: foreground app is unknown.",
            );
        };

        let Some((rule_process_name, requested_100ns)) =
            settings.desired_resolution_for_foreground(foreground_executable_path)
        else {
            return self.release_inactive(
                controller,
                true,
                Some(info),
                action_log,
                "no foreground timer resolution rule matched",
                "Waiting for a matching foreground app.",
            );
        };

        let desired_100ns =
            normalize_desired_resolution(requested_100ns, info.minimum_100ns, info.maximum_100ns);

        let previous_target = self
            .active_rule_process
            .clone()
            .unwrap_or_else(|| SYSTEM_TARGET_NAME.to_owned());
        match controller.set_request(desired_100ns) {
            Ok(transition) => {
                if let Some(previous_100ns) = transition.released_100ns {
                    action_log.record(
                        ActionLogFeature::TimerResolution,
                        None,
                        previous_target,
                        ActionLogResult::Restored,
                        format!(
                            "Released previous timer resolution request {}.",
                            format_resolution_ms(previous_100ns)
                        ),
                    );
                }
                if transition.changed {
                    action_log.record(
                        ActionLogFeature::TimerResolution,
                        None,
                        rule_process_name.clone(),
                        ActionLogResult::Applied,
                        format!(
                            "Requested timer resolution {} while {} is foreground.",
                            format_resolution_ms(transition.active_100ns),
                            rule_process_name
                        ),
                    );
                }
                self.active_rule_process = Some(rule_process_name.clone());
            }
            Err(error) => {
                if let Some(previous_100ns) = error.released_100ns {
                    action_log.record(
                        ActionLogFeature::TimerResolution,
                        None,
                        previous_target.clone(),
                        ActionLogResult::Restored,
                        format!(
                            "Released previous timer resolution request {}.",
                            format_resolution_ms(previous_100ns)
                        ),
                    );
                }

                let (failed_target, action_message, snapshot_message) = match error.stage {
                    TimerResolutionTransitionStage::ReleasePrevious => (
                        previous_target,
                        format!(
                            "Failed to release previous timer resolution request: {}",
                            error.message
                        ),
                        "Timer resolution request update failed.",
                    ),
                    TimerResolutionTransitionStage::RequestDesired => {
                        self.active_rule_process = None;
                        (
                            rule_process_name,
                            error.message.clone(),
                            "Timer resolution request failed.",
                        )
                    }
                };
                action_log.record(
                    ActionLogFeature::TimerResolution,
                    None,
                    failed_target,
                    ActionLogResult::Failed,
                    action_message,
                );
                return snapshot_from_query(
                    true,
                    error.active_100ns,
                    self.active_rule_process.clone(),
                    Some(info),
                    1,
                    Some(error.message),
                    snapshot_message,
                );
            }
        }

        snapshot_from_query(
            true,
            controller.active_request_100ns(),
            Some(rule_process_name),
            Some(info),
            0,
            None,
            "Timer resolution request active.",
        )
    }

    fn disable(
        &mut self,
        controller: &mut TimerResolutionController,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> TimerResolutionSnapshot {
        let info = controller.query().ok();
        self.release_inactive(
            controller,
            false,
            info,
            action_log,
            reason,
            "Timer resolution control disabled.",
        )
    }

    fn release_inactive(
        &mut self,
        controller: &mut TimerResolutionController,
        enabled: bool,
        info: Option<TimerResolutionInfo>,
        action_log: &mut ActionLog,
        reason: &str,
        message: &str,
    ) -> TimerResolutionSnapshot {
        let mut failures = 0;
        let mut last_error = None;
        if let Some(previous_100ns) = controller.active_request_100ns() {
            let previous_target = self
                .active_rule_process
                .clone()
                .unwrap_or_else(|| SYSTEM_TARGET_NAME.to_owned());
            match controller.release() {
                Ok(_) => {
                    self.active_rule_process = None;
                    action_log.record(
                        ActionLogFeature::TimerResolution,
                        None,
                        previous_target,
                        ActionLogResult::Restored,
                        format!(
                            "Released timer resolution request {}: {reason}.",
                            format_resolution_ms(previous_100ns)
                        ),
                    );
                }
                Err(message) => {
                    failures += 1;
                    last_error = Some(message.clone());
                    action_log.record(
                        ActionLogFeature::TimerResolution,
                        None,
                        previous_target,
                        ActionLogResult::Failed,
                        message,
                    );
                }
            }
        }

        snapshot_from_query(
            enabled,
            controller.active_request_100ns(),
            self.active_rule_process.clone(),
            info,
            failures,
            last_error,
            message,
        )
    }
}

pub fn query_snapshot(enabled: bool) -> TimerResolutionSnapshot {
    match TimerResolutionController::default().query() {
        Ok(info) => snapshot_from_query(
            enabled,
            None,
            None,
            Some(info),
            0,
            None,
            if enabled {
                "Timer resolution status loaded."
            } else {
                "Timer resolution control disabled."
            },
        ),
        Err(err) => {
            let message = err;
            TimerResolutionSnapshot {
                enabled,
                failed_actions: 1,
                message: "Timer resolution query failed.".to_owned(),
                last_error: Some(message),
                ..Default::default()
            }
        }
    }
}

pub fn normalize_desired_resolution(
    desired_100ns: u32,
    minimum_100ns: u32,
    maximum_100ns: u32,
) -> u32 {
    let min_100ns = minimum_100ns.min(maximum_100ns).max(1);
    let max_100ns = minimum_100ns.max(maximum_100ns).max(min_100ns);
    let clamped_100ns = desired_100ns.clamp(min_100ns, max_100ns);
    period_ms_to_100ns(resolution_100ns_to_period_ms(clamped_100ns)).clamp(min_100ns, max_100ns)
}

pub fn format_resolution_ms(value_100ns: u32) -> String {
    let milliseconds = value_100ns as f64 / 10_000.0;
    if milliseconds >= 10.0 {
        format!("{milliseconds:.3} ms")
    } else if milliseconds >= 1.0 {
        format!("{milliseconds:.2} ms")
    } else {
        format!("{milliseconds:.3} ms")
    }
}

fn snapshot_from_query(
    enabled: bool,
    requested_100ns: Option<u32>,
    active_rule_process: Option<String>,
    info: Option<TimerResolutionInfo>,
    failed_actions: usize,
    last_error: Option<String>,
    message: &str,
) -> TimerResolutionSnapshot {
    TimerResolutionSnapshot {
        enabled,
        requested_100ns,
        active_rule_process,
        maximum_100ns: info.map(|info| info.maximum_100ns),
        minimum_100ns: info.map(|info| info.minimum_100ns),
        failed_actions,
        message: message.to_owned(),
        last_error,
    }
}

fn resolution_100ns_to_period_ms(value_100ns: u32) -> u32 {
    value_100ns.div_ceil(10_000).max(1)
}

fn period_ms_to_100ns(period_ms: u32) -> u32 {
    period_ms.saturating_mul(10_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desired_resolution_is_clamped_between_minimum_and_maximum() {
        assert_eq!(normalize_desired_resolution(1_000, 10_000, 160_000), 10_000);
        assert_eq!(
            normalize_desired_resolution(200_000, 10_000, 160_000),
            160_000
        );
        assert_eq!(
            normalize_desired_resolution(10_000, 10_000, 160_000),
            10_000
        );
    }

    #[test]
    fn desired_resolution_rounds_up_to_whole_milliseconds() {
        assert_eq!(
            normalize_desired_resolution(15_500, 10_000, 160_000),
            20_000
        );
    }

    #[test]
    fn resolution_format_uses_milliseconds() {
        assert_eq!(format_resolution_ms(160_000), "16.000 ms");
        assert_eq!(format_resolution_ms(20_000), "2.00 ms");
        assert_eq!(format_resolution_ms(10_000), "1.00 ms");
    }
}
