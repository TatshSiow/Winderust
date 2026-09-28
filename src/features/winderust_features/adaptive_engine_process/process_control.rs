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

use super::BACKGROUND_APPLY_SUMMARY_LOG_INTERVAL;

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

pub(super) fn process_age(process_id: u32) -> Option<Duration> {
    let process = ProcessHandle::open_query(process_id)?;
    let creation_time_100ns = process.creation_time_100ns()?;
    let mut now = FILETIME::default();
    // SAFETY: now is writable FILETIME storage for the duration of the call.
    unsafe {
        GetSystemTimeAsFileTime(&mut now);
    }
    let age_100ns = filetime_to_u64(now).saturating_sub(creation_time_100ns);
    Some(Duration::from_nanos(age_100ns.saturating_mul(100)))
}

pub(super) fn process_group_cpu_sample(identities: &[(u32, u64)]) -> Option<ProcessCpuSample> {
    sample_process_group(identities, |id| {
        ProcessHandle::open_query(id)?.cpu_sample_with_identity()
    })
}

fn sample_process_group(
    identities: &[(u32, u64)],
    mut sample: impl FnMut(u32) -> Option<(ProcessCpuSample, u64)>,
) -> Option<ProcessCpuSample> {
    if identities.is_empty() {
        return None;
    }
    let sampled_at = Instant::now();
    let mut cpu_time_100ns = 0u64;
    for &(id, creation) in identities {
        let (sample, actual_creation) = sample(id)?;
        if creation != actual_creation {
            return None;
        }
        cpu_time_100ns = cpu_time_100ns.checked_add(sample.cpu_time_100ns)?;
    }
    Some(ProcessCpuSample {
        cpu_time_100ns,
        sampled_at,
    })
}

#[derive(Default)]
pub(super) struct PriorityFailures {
    pub(super) count: usize,
    pub(super) last_error: Option<String>,
}

impl PriorityFailures {
    pub(super) fn merge(&mut self, other: Self) {
        self.count += other.count;
        if self.last_error.is_none() {
            self.last_error = other.last_error;
        }
    }

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

pub(super) fn background_apply_summary_message(count: usize) -> String {
    if count == 1 {
        "Background limits updated for 1 process.".to_owned()
    } else {
        format!("Background limits updated for {count} processes.")
    }
}

pub(super) fn background_apply_summary_log_due(
    last_logged_at: Option<Instant>,
    now: Instant,
) -> bool {
    last_logged_at
        .is_none_or(|last| now.duration_since(last) >= BACKGROUND_APPLY_SUMMARY_LOG_INTERVAL)
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn group_samples_require_every_exact_member() {
        let sample = |time| ProcessCpuSample {
            cpu_time_100ns: time,
            sampled_at: Instant::now(),
        };
        let group = [(1, 10), (2, 20)];
        assert!(sample_process_group(&group, |id| (id == 1).then(|| (sample(100), 10))).is_none());
        assert!(sample_process_group(&group, |id| Some((
            sample(100),
            if id == 1 { 10 } else { 21 }
        )))
        .is_none());
        assert_eq!(
            sample_process_group(&group, |id| Some((
                sample(100 * u64::from(id)),
                10 * u64::from(id)
            )))
            .unwrap()
            .cpu_time_100ns,
            300
        );
    }
}
