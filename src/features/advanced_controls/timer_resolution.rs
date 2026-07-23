use crate::{
    action_log::{ActionLog, ActionLogFeature, ActionLogResult},
    config::TimerResolutionSettings,
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
    active_request_100ns: Option<u32>,
    active_rule_process: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimerResolutionInfo {
    pub maximum_100ns: u32,
    pub minimum_100ns: u32,
}

impl TimerResolutionManager {
    pub fn update(
        &mut self,
        settings: &TimerResolutionSettings,
        automation_enabled: bool,
        foreground_process_name: Option<&str>,
        action_log: &mut ActionLog,
    ) -> TimerResolutionSnapshot {
        if !automation_enabled {
            return self.disable(action_log, "automation disabled");
        }

        if !settings.enabled {
            return self.disable(action_log, "timer resolution control disabled");
        }

        let info = match query_timer_resolution() {
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

        let Some(foreground_process_name) =
            foreground_process_name.filter(|name| !name.trim().is_empty())
        else {
            return self.release_inactive(
                true,
                Some(info),
                action_log,
                "foreground app is unknown",
                "Paused: foreground app is unknown.",
            );
        };

        let Some((rule_process_name, requested_100ns)) =
            settings.desired_resolution_for_foreground(foreground_process_name)
        else {
            let message = if settings.rules.iter().any(|rule| rule.enabled) {
                "Waiting for a matching foreground app."
            } else {
                "No timer resolution foreground rules configured."
            };
            return self.release_inactive(
                true,
                Some(info),
                action_log,
                "no foreground timer resolution rule matched",
                message,
            );
        };

        let desired_100ns =
            normalize_desired_resolution(requested_100ns, info.minimum_100ns, info.maximum_100ns);

        if self.active_request_100ns != Some(desired_100ns) {
            if let Some(previous_100ns) = self.active_request_100ns.take() {
                let previous_target = self
                    .active_rule_process
                    .take()
                    .unwrap_or_else(|| SYSTEM_TARGET_NAME.to_owned());
                if let Err(err) = release_timer_resolution(previous_100ns) {
                    let message = err;
                    action_log.record(
                        ActionLogFeature::TimerResolution,
                        None,
                        previous_target,
                        ActionLogResult::Failed,
                        format!("Failed to release previous timer resolution request: {message}"),
                    );
                    return snapshot_from_query(
                        true,
                        None,
                        None,
                        Some(info),
                        1,
                        Some(message),
                        "Timer resolution request update failed.",
                    );
                }

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

            match request_timer_resolution(desired_100ns) {
                Ok(applied_100ns) => {
                    self.active_request_100ns = Some(applied_100ns);
                    action_log.record(
                        ActionLogFeature::TimerResolution,
                        None,
                        rule_process_name.clone(),
                        ActionLogResult::Applied,
                        format!(
                            "Requested timer resolution {} while {} is foreground.",
                            format_resolution_ms(applied_100ns),
                            rule_process_name
                        ),
                    );
                    self.active_rule_process = Some(rule_process_name.clone());
                }
                Err(err) => {
                    let message = err;
                    action_log.record(
                        ActionLogFeature::TimerResolution,
                        None,
                        rule_process_name,
                        ActionLogResult::Failed,
                        message.clone(),
                    );
                    return snapshot_from_query(
                        true,
                        None,
                        None,
                        Some(info),
                        1,
                        Some(message),
                        "Timer resolution request failed.",
                    );
                }
            }
        } else {
            self.active_rule_process = Some(rule_process_name.clone());
        }

        let requested_100ns = self.active_request_100ns;
        snapshot_from_query(
            true,
            requested_100ns,
            Some(rule_process_name),
            Some(info),
            0,
            None,
            "Timer resolution request active.",
        )
    }

    fn disable(&mut self, action_log: &mut ActionLog, reason: &str) -> TimerResolutionSnapshot {
        let info = query_timer_resolution().ok();
        self.release_inactive(
            false,
            info,
            action_log,
            reason,
            "Timer resolution control disabled.",
        )
    }

    fn release_inactive(
        &mut self,
        enabled: bool,
        info: Option<TimerResolutionInfo>,
        action_log: &mut ActionLog,
        reason: &str,
        message: &str,
    ) -> TimerResolutionSnapshot {
        let mut failures = 0;
        let mut last_error = None;
        if let Some(previous_100ns) = self.active_request_100ns.take() {
            let previous_target = self
                .active_rule_process
                .take()
                .unwrap_or_else(|| SYSTEM_TARGET_NAME.to_owned());
            match release_timer_resolution(previous_100ns) {
                Ok(()) => {
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
                Err(err) => {
                    failures += 1;
                    let message = err;
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
        self.active_rule_process = None;

        snapshot_from_query(enabled, None, None, info, failures, last_error, message)
    }
}

impl Drop for TimerResolutionManager {
    fn drop(&mut self) {
        if let Some(previous_100ns) = self.active_request_100ns.take() {
            let _ = release_timer_resolution(previous_100ns);
        }
        self.active_rule_process = None;
    }
}

pub fn query_snapshot(enabled: bool) -> TimerResolutionSnapshot {
    match query_timer_resolution() {
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

fn query_timer_resolution() -> Result<TimerResolutionInfo, String> {
    let mut caps = TimeCaps::default();
    // SAFETY: caps is writable for exactly the supplied TimeCaps size and the FFI declaration
    // matches timeGetDevCaps.
    let result =
        unsafe { time_get_dev_caps(&mut caps as *mut _, std::mem::size_of::<TimeCaps>() as u32) };
    mm_result("timeGetDevCaps", result)?;

    let min_period_ms = caps.period_min.max(1);
    let max_period_ms = caps.period_max.max(min_period_ms);
    Ok(TimerResolutionInfo {
        maximum_100ns: period_ms_to_100ns(max_period_ms),
        minimum_100ns: period_ms_to_100ns(min_period_ms),
    })
}

fn request_timer_resolution(desired_100ns: u32) -> Result<u32, String> {
    let period_ms = resolution_100ns_to_period_ms(desired_100ns);
    // SAFETY: period_ms is normalized to a positive millisecond period accepted by winmm.
    let result = unsafe { time_begin_period(period_ms) };
    mm_result("timeBeginPeriod", result).map(|()| period_ms_to_100ns(period_ms))
}

fn release_timer_resolution(desired_100ns: u32) -> Result<(), String> {
    let period_ms = resolution_100ns_to_period_ms(desired_100ns);
    // SAFETY: period_ms is the same normalized value used for the matching begin request.
    let result = unsafe { time_end_period(period_ms) };
    mm_result("timeEndPeriod", result)
}

fn resolution_100ns_to_period_ms(value_100ns: u32) -> u32 {
    value_100ns.div_ceil(10_000).max(1)
}

fn period_ms_to_100ns(period_ms: u32) -> u32 {
    period_ms.saturating_mul(10_000)
}

fn mm_result(operation: &str, result: u32) -> Result<(), String> {
    if result == MMSYSERR_NOERROR {
        Ok(())
    } else {
        Err(format!("{operation} failed with MMRESULT {result}."))
    }
}

const MMSYSERR_NOERROR: u32 = 0;

#[repr(C)]
#[derive(Default)]
struct TimeCaps {
    period_min: u32,
    period_max: u32,
}

#[link(name = "winmm")]
unsafe extern "system" {
    #[link_name = "timeGetDevCaps"]
    fn time_get_dev_caps(ptc: *mut TimeCaps, cbtc: u32) -> u32;
    #[link_name = "timeBeginPeriod"]
    fn time_begin_period(u_period: u32) -> u32;
    #[link_name = "timeEndPeriod"]
    fn time_end_period(u_period: u32) -> u32;
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
