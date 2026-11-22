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
    pub maximum_100ns: Option<u32>,
    pub minimum_100ns: Option<u32>,
    pub observation_paused: bool,
    pub last_error: Option<String>,
}

#[derive(Default)]
pub struct TimerResolutionManager {
    active_rule_process: Option<String>,
    last_release_error: Option<String>,
    last_query_error: Option<String>,
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
            return self.disable(
                controller,
                action_log,
                "timer_resolution.log_automation_disabled",
            );
        }

        if !settings.enabled {
            return self.disable(
                controller,
                action_log,
                "timer_resolution.log_control_disabled",
            );
        }

        if !settings.rules.iter().any(|rule| {
            rule.enabled && std::path::Path::new(rule.executable_path.trim()).is_absolute()
        }) {
            return self.release_inactive(
                controller,
                true,
                false,
                action_log,
                "timer_resolution.log_no_active_rules",
                false,
            );
        }

        let Some(foreground_executable_path) =
            foreground_executable_path.filter(|path| !path.trim().is_empty())
        else {
            return self.release_inactive(
                controller,
                true,
                true,
                action_log,
                "timer_resolution.log_unknown_foreground",
                true,
            );
        };

        let Some((rule_process_name, requested_100ns)) =
            settings.desired_resolution_for_foreground(foreground_executable_path)
        else {
            return self.release_inactive(
                controller,
                true,
                true,
                action_log,
                "timer_resolution.log_no_match",
                false,
            );
        };

        let info = match controller.query() {
            Ok(info) => {
                self.last_query_error = None;
                info
            }
            Err(error) => {
                record_query_failure(&mut self.last_query_error, action_log, &error);
                return TimerResolutionSnapshot {
                    enabled: true,
                    requested_100ns: controller.active_request_100ns(),
                    last_error: Some(error),
                    ..Default::default()
                };
            }
        };

        let desired_100ns =
            normalize_desired_resolution(requested_100ns, info.minimum_100ns, info.maximum_100ns);

        let previous_target = self
            .active_rule_process
            .as_deref()
            .unwrap_or(SYSTEM_TARGET_NAME);
        let result = controller.set_request(desired_100ns);
        let released = match &result {
            Ok(transition) => transition.released_100ns,
            Err(error) => error.released_100ns,
        };
        if let Some(previous_100ns) = released {
            self.last_release_error = None;
            action_log.record(
                ActionLogFeature::TimerResolution,
                None,
                previous_target,
                ActionLogResult::Restored,
                rust_i18n::t!(
                    "timer_resolution.log_released_previous",
                    resolution = format_resolution_ms(previous_100ns)
                ),
            );
        }
        match result {
            Ok(transition) => {
                if transition.changed {
                    action_log.record(
                        ActionLogFeature::TimerResolution,
                        None,
                        rule_process_name,
                        ActionLogResult::Applied,
                        rust_i18n::t!(
                            "timer_resolution.log_requested",
                            resolution = format_resolution_ms(transition.active_100ns),
                            app = rule_process_name
                        ),
                    );
                }
                if self.active_rule_process.as_deref() != Some(rule_process_name) {
                    self.active_rule_process = Some(rule_process_name.to_owned());
                }
            }
            Err(error) => {
                let failure = match error.stage {
                    TimerResolutionTransitionStage::ReleasePrevious
                        if remember_error(&mut self.last_release_error, &error.message) =>
                    {
                        Some((
                            previous_target,
                            rust_i18n::t!(
                                "timer_resolution.log_release_previous_failed",
                                error = error.message
                            )
                            .into_owned(),
                        ))
                    }
                    TimerResolutionTransitionStage::ReleasePrevious => None,
                    TimerResolutionTransitionStage::RequestDesired => {
                        self.active_rule_process = None;
                        Some((rule_process_name, error.message.clone()))
                    }
                };
                if let Some((target, reason)) = failure {
                    action_log.record(
                        ActionLogFeature::TimerResolution,
                        None,
                        target,
                        ActionLogResult::Failed,
                        reason,
                    );
                }
                return snapshot_from_query(
                    true,
                    error.active_100ns,
                    Some(info),
                    Some(error.message),
                    false,
                );
            }
        }

        snapshot_from_query(
            true,
            controller.active_request_100ns(),
            Some(info),
            None,
            false,
        )
    }

    fn disable(
        &mut self,
        controller: &mut TimerResolutionController,
        action_log: &mut ActionLog,
        reason_key: &str,
    ) -> TimerResolutionSnapshot {
        self.last_query_error = None;
        self.release_inactive(controller, false, true, action_log, reason_key, false)
    }

    fn release_inactive(
        &mut self,
        controller: &mut TimerResolutionController,
        enabled: bool,
        refresh_bounds: bool,
        action_log: &mut ActionLog,
        reason_key: &str,
        observation_paused: bool,
    ) -> TimerResolutionSnapshot {
        let mut last_error = None;
        if let Some(previous_100ns) = controller.active_request_100ns() {
            match controller.release() {
                Ok(_) => {
                    let previous_target = self
                        .active_rule_process
                        .take()
                        .unwrap_or_else(|| SYSTEM_TARGET_NAME.to_owned());
                    self.last_release_error = None;
                    action_log.record(
                        ActionLogFeature::TimerResolution,
                        None,
                        previous_target,
                        ActionLogResult::Restored,
                        rust_i18n::t!(
                            "timer_resolution.log_released",
                            resolution = format_resolution_ms(previous_100ns),
                            reason = rust_i18n::t!(reason_key)
                        ),
                    );
                }
                Err(message) => {
                    if remember_error(&mut self.last_release_error, &message) {
                        action_log.record(
                            ActionLogFeature::TimerResolution,
                            None,
                            self.active_rule_process
                                .as_deref()
                                .unwrap_or(SYSTEM_TARGET_NAME),
                            ActionLogResult::Failed,
                            message.clone(),
                        );
                    }
                    last_error = Some(message);
                }
            }
        }

        // Releasing the exact owned request does not depend on current device capabilities.
        let info = if refresh_bounds {
            match controller.query() {
                Ok(info) => {
                    self.last_query_error = None;
                    Some(info)
                }
                Err(error) if enabled && last_error.is_none() => {
                    record_query_failure(&mut self.last_query_error, action_log, &error);
                    last_error = Some(error);
                    None
                }
                Err(_) => None,
            }
        } else {
            None
        };
        snapshot_from_query(
            enabled,
            controller.active_request_100ns(),
            info,
            last_error,
            observation_paused,
        )
    }
}

fn remember_error(last_error: &mut Option<String>, error: &str) -> bool {
    if last_error.as_deref() == Some(error) {
        return false;
    }
    *last_error = Some(error.to_owned());
    true
}

fn record_query_failure(last_error: &mut Option<String>, action_log: &mut ActionLog, error: &str) {
    if !remember_error(last_error, error) {
        return;
    }
    action_log.record(
        ActionLogFeature::TimerResolution,
        None,
        SYSTEM_TARGET_NAME,
        ActionLogResult::Failed,
        error.to_owned(),
    );
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
    info: Option<TimerResolutionInfo>,
    last_error: Option<String>,
    observation_paused: bool,
) -> TimerResolutionSnapshot {
    TimerResolutionSnapshot {
        enabled,
        requested_100ns,
        maximum_100ns: info.map(|info| info.maximum_100ns),
        minimum_100ns: info.map(|info| info.minimum_100ns),
        observation_paused,
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
