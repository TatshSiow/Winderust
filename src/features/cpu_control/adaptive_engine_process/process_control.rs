use std::{path::Path, time::Duration, time::Instant};

use windows_sys::Win32::{
    Foundation::FILETIME,
    System::{
        SystemInformation::GetSystemTimeAsFileTime,
        Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    },
};

use crate::{
    action_log::{ActionLog, ActionLogFeature, ActionLogResult},
    control::process::ProcessControlError,
    cpu::ProcessCpuSample,
    foreground::process_handle_matches_executable_path,
    win_util::{filetime_to_u64, WinHandle},
};

pub(super) fn process_cpu_sample_with_identity(
    process_id: u32,
    executable_path: &str,
) -> Option<(ProcessCpuSample, u64)> {
    let process = ProcessHandle::open_query(process_id)?;
    if !process.matches_executable_path(executable_path) {
        return None;
    }
    process.cpu_sample_with_identity()
}

pub(super) fn process_age(process_id: u32, expected_creation_time: u64) -> Option<Duration> {
    let process = ProcessHandle::open_query(process_id)?;
    let creation_time_100ns = process.creation_time_100ns()?;
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

struct ProcessHandle(WinHandle);

impl ProcessHandle {
    fn open_query(process_id: u32) -> Option<Self> {
        // SAFETY: process_id came from the current process snapshot and no inherited handle is
        // requested.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
        if !handle.is_null() {
            Some(Self(WinHandle::new(handle)))
        } else {
            None
        }
    }

    fn matches_executable_path(&self, expected: &str) -> bool {
        process_handle_matches_executable_path(&self.0, Path::new(expected))
    }

    fn cpu_sample_with_identity(&self) -> Option<(ProcessCpuSample, u64)> {
        let (creation, cpu_time_100ns) = self.0.process_times().ok()?;
        Some((
            ProcessCpuSample {
                cpu_time_100ns,
                sampled_at: Instant::now(),
            },
            creation,
        ))
    }

    fn creation_time_100ns(&self) -> Option<u64> {
        self.0.process_creation_time()
    }
}
