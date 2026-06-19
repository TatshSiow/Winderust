use crate::{
    action_log::{ActionLog, ActionLogAction, ActionLogFeature, ActionLogResult},
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
    pub current_100ns: Option<u32>,
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
    pub current_100ns: u32,
}

#[derive(Debug)]
enum TimerResolutionError {
    Failed(String),
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
                let message = timer_resolution_error_message(err);
                action_log.record(
                    ActionLogFeature::TimerResolution,
                    None,
                    SYSTEM_TARGET_NAME,
                    ActionLogAction::Fail,
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
                    let message = timer_resolution_error_message(err);
                    action_log.record(
                        ActionLogFeature::TimerResolution,
                        None,
                        previous_target,
                        ActionLogAction::Fail,
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
                    ActionLogAction::Restore,
                    ActionLogResult::Restored,
                    format!(
                        "Released previous timer resolution request {}.",
                        format_resolution_ms(previous_100ns)
                    ),
                );
            }

            match request_timer_resolution(desired_100ns) {
                Ok(current_100ns) => {
                    self.active_request_100ns = Some(desired_100ns);
                    action_log.record(
                        ActionLogFeature::TimerResolution,
                        None,
                        rule_process_name.clone(),
                        ActionLogAction::Apply,
                        ActionLogResult::Applied,
                        format!(
                            "Requested timer resolution {} while {} is foreground; current is {}.",
                            format_resolution_ms(desired_100ns),
                            rule_process_name,
                            format_resolution_ms(current_100ns)
                        ),
                    );
                    self.active_rule_process = Some(rule_process_name.clone());
                }
                Err(err) => {
                    let message = timer_resolution_error_message(err);
                    action_log.record(
                        ActionLogFeature::TimerResolution,
                        None,
                        rule_process_name,
                        ActionLogAction::Fail,
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

        let refreshed = query_timer_resolution().unwrap_or(info);
        snapshot_from_query(
            true,
            Some(desired_100ns),
            Some(rule_process_name),
            Some(refreshed),
            0,
            None,
            "Timer resolution request active.",
        )
    }

    fn disable(&mut self, action_log: &mut ActionLog, reason: &str) -> TimerResolutionSnapshot {
        let mut failures = 0;
        let mut last_error = None;
        if let Some(previous_100ns) = self.active_request_100ns.take() {
            let previous_target = self
                .active_rule_process
                .take()
                .unwrap_or_else(|| SYSTEM_TARGET_NAME.to_owned());
            match release_timer_resolution(previous_100ns) {
                Ok(current_100ns) => {
                    action_log.record(
                        ActionLogFeature::TimerResolution,
                        None,
                        previous_target,
                        ActionLogAction::Restore,
                        ActionLogResult::Restored,
                        format!(
                            "Released timer resolution request {}: {reason}. Current is {}.",
                            format_resolution_ms(previous_100ns),
                            format_resolution_ms(current_100ns)
                        ),
                    );
                }
                Err(err) => {
                    failures += 1;
                    let message = timer_resolution_error_message(err);
                    last_error = Some(message.clone());
                    action_log.record(
                        ActionLogFeature::TimerResolution,
                        None,
                        previous_target,
                        ActionLogAction::Fail,
                        ActionLogResult::Failed,
                        message,
                    );
                }
            }
        }
        self.active_rule_process = None;

        let info = query_timer_resolution().ok();
        snapshot_from_query(
            false,
            None,
            None,
            info,
            failures,
            last_error,
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
                Ok(current_100ns) => {
                    action_log.record(
                        ActionLogFeature::TimerResolution,
                        None,
                        previous_target,
                        ActionLogAction::Restore,
                        ActionLogResult::Restored,
                        format!(
                            "Released timer resolution request {}: {reason}. Current is {}.",
                            format_resolution_ms(previous_100ns),
                            format_resolution_ms(current_100ns)
                        ),
                    );
                }
                Err(err) => {
                    failures += 1;
                    let message = timer_resolution_error_message(err);
                    last_error = Some(message.clone());
                    action_log.record(
                        ActionLogFeature::TimerResolution,
                        None,
                        previous_target,
                        ActionLogAction::Fail,
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
            let message = timer_resolution_error_message(err);
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
    desired_100ns.clamp(
        minimum_100ns.min(maximum_100ns),
        minimum_100ns.max(maximum_100ns),
    )
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
        current_100ns: info.map(|info| info.current_100ns),
        failed_actions,
        message: message.to_owned(),
        last_error,
    }
}

fn query_timer_resolution() -> Result<TimerResolutionInfo, TimerResolutionError> {
    let mut maximum_100ns = 0_u32;
    let mut minimum_100ns = 0_u32;
    let mut current_100ns = 0_u32;
    let status = unsafe {
        NtQueryTimerResolution(
            &mut maximum_100ns as *mut _,
            &mut minimum_100ns as *mut _,
            &mut current_100ns as *mut _,
        )
    };
    ntstatus_result(status).map(|()| TimerResolutionInfo {
        maximum_100ns,
        minimum_100ns,
        current_100ns,
    })
}

fn request_timer_resolution(desired_100ns: u32) -> Result<u32, TimerResolutionError> {
    set_timer_resolution(desired_100ns, true)
}

fn release_timer_resolution(desired_100ns: u32) -> Result<u32, TimerResolutionError> {
    set_timer_resolution(desired_100ns, false)
}

fn set_timer_resolution(
    desired_100ns: u32,
    set_resolution: bool,
) -> Result<u32, TimerResolutionError> {
    let mut current_100ns = 0_u32;
    let status = unsafe {
        NtSetTimerResolution(
            desired_100ns,
            u8::from(set_resolution),
            &mut current_100ns as *mut _,
        )
    };
    ntstatus_result(status).map(|()| current_100ns)
}

fn ntstatus_result(status: i32) -> Result<(), TimerResolutionError> {
    if status >= 0 {
        Ok(())
    } else {
        Err(TimerResolutionError::Failed(format!(
            "NTSTATUS 0x{:08X}.",
            status as u32
        )))
    }
}

fn timer_resolution_error_message(error: TimerResolutionError) -> String {
    match error {
        TimerResolutionError::Failed(message) => message,
    }
}

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtQueryTimerResolution(
        MaximumTime: *mut u32,
        MinimumTime: *mut u32,
        CurrentTime: *mut u32,
    ) -> i32;

    fn NtSetTimerResolution(
        DesiredResolution: u32,
        SetResolution: u8,
        CurrentResolution: *mut u32,
    ) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desired_resolution_is_clamped_between_minimum_and_maximum() {
        assert_eq!(normalize_desired_resolution(1_000, 5_000, 156_250), 5_000);
        assert_eq!(
            normalize_desired_resolution(200_000, 5_000, 156_250),
            156_250
        );
        assert_eq!(normalize_desired_resolution(10_000, 5_000, 156_250), 10_000);
    }

    #[test]
    fn resolution_format_uses_milliseconds() {
        assert_eq!(format_resolution_ms(156_250), "15.625 ms");
        assert_eq!(format_resolution_ms(10_000), "1.00 ms");
        assert_eq!(format_resolution_ms(5_000), "0.500 ms");
    }
}
