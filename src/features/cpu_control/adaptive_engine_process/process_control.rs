use std::time::{Duration, Instant};

use windows_sys::Win32::{
    Foundation::FILETIME, System::SystemInformation::GetSystemTimeAsFileTime,
};

use crate::{
    action_log::{ActionLog, ActionLogFeature, ActionLogResult},
    control::process::ProcessControlError,
    cpu::ProcessCpuSample,
    foreground::process_list::process_cpu_times,
    win_util::filetime_to_u64,
};

pub(super) fn process_cpu_sample(process_id: u32) -> Option<(ProcessCpuSample, u64)> {
    let (creation_time, cpu_time_100ns) = process_cpu_times(process_id)?;
    Some((
        ProcessCpuSample {
            cpu_time_100ns,
            sampled_at: Instant::now(),
        },
        creation_time,
    ))
}

pub(super) fn process_age(process_id: u32, expected_creation_time: u64) -> Option<Duration> {
    let (creation_time_100ns, _) = process_cpu_times(process_id)?;
    if creation_time_100ns != expected_creation_time {
        return None;
    }
    let mut now = FILETIME::default();
    // SAFETY: now is writable FILETIME storage for the duration of the call.
    unsafe {
        GetSystemTimeAsFileTime(&mut now);
    }
    let age_100ns = filetime_to_u64(now).saturating_sub(creation_time_100ns);
    Some(Duration::from_nanos(age_100ns.saturating_mul(100)))
}

#[derive(Default)]
pub(super) struct PriorityFailures {
    pub(super) count: usize,
    pub(super) last_error: Option<String>,
}

impl PriorityFailures {
    pub(super) fn record_control_error(
        &mut self,
        action: &str,
        process_id: u32,
        process_name: &str,
        error: ProcessControlError,
        action_log: &mut ActionLog,
    ) {
        if error == ProcessControlError::ProcessExited {
            return;
        }
        self.record_message(
            action,
            process_id,
            process_name,
            error.to_string(),
            action_log,
        );
    }

    pub(super) fn record_message(
        &mut self,
        action: &str,
        process_id: u32,
        process_name: &str,
        message: String,
        action_log: &mut ActionLog,
    ) {
        self.count += 1;
        if self.last_error.is_none() {
            self.last_error = Some(process_failure_message(
                action,
                process_id,
                process_name,
                &message,
            ));
        }
        action_log.record(
            ActionLogFeature::AdaptiveEngine,
            Some(process_id),
            process_name.to_owned(),
            ActionLogResult::Failed,
            message,
        );
    }
}

pub(super) fn process_failure_message(
    action: &str,
    process_id: u32,
    process_name: &str,
    message: &str,
) -> String {
    let name = if process_name.is_empty() {
        "process"
    } else {
        process_name
    };
    format!("{action} {name} ({process_id}): {message}")
}
