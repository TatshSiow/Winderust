use std::{collections::BTreeSet, path::Path, time::Duration, time::Instant};

use windows_sys::Win32::{
    Foundation::FILETIME,
    System::{
        SystemInformation::GetSystemTimeAsFileTime,
        Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    },
};

use crate::{
    action_log::{ActionLog, ActionLogFeature, ActionLogResult},
    control::process::ProcessControlError,
    cpu::ProcessCpuSample,
    foreground::process_handle_matches_executable_path,
    win_util::{filetime_to_u64, WinHandle},
};

use super::{PriorityTargetSource, BACKGROUND_APPLY_SUMMARY_LOG_INTERVAL};

pub(super) fn process_cpu_sample(process_id: u32) -> Option<ProcessCpuSample> {
    let process = ProcessHandle::open_query(process_id)?;
    process.cpu_sample()
}

pub(super) fn process_cpu_sample_with_identity(
    process_id: u32,
    executable_path: &str,
) -> Option<(ProcessCpuSample, u64)> {
    let process = ProcessHandle::open_query(process_id)?;
    if !process.matches_executable_path(executable_path) {
        return None;
    }
    let creation_time = process.creation_time_100ns()?;
    Some((process.cpu_sample()?, creation_time))
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

pub(super) fn process_group_cpu_sample(process_ids: &BTreeSet<u32>) -> Option<ProcessCpuSample> {
    let sampled_at = Instant::now();
    let mut cpu_time_100ns = 0u64;
    let mut sampled_any = false;
    for process_id in process_ids {
        let Some(sample) = process_cpu_sample(*process_id) else {
            continue;
        };
        cpu_time_100ns = cpu_time_100ns.saturating_add(sample.cpu_time_100ns);
        sampled_any = true;
    }

    sampled_any.then_some(ProcessCpuSample {
        cpu_time_100ns,
        sampled_at,
    })
}

pub(super) fn ignore_timer_resolution_allowed(
    process_id: u32,
    active_audio_process_ids: Option<&BTreeSet<u32>>,
) -> bool {
    active_audio_process_ids.is_some_and(|ids| !ids.contains(&process_id))
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
            ActionLogFeature::WorkloadEngine,
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

pub(super) fn priority_source_label(source: PriorityTargetSource) -> &'static str {
    match source {
        PriorityTargetSource::WorkloadEngine => "Workload Engine",
        PriorityTargetSource::BackgroundPolicy => "Background policy",
        PriorityTargetSource::VisibleWindow => "Visible window",
        PriorityTargetSource::Rule => "Rule",
    }
}

pub(super) fn background_apply_summary_message(count: usize) -> String {
    if count == 1 {
        "Applied Workload Engine background restraint to 1 process.".to_owned()
    } else {
        format!("Applied Workload Engine background restraint to {count} processes.")
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

    fn cpu_sample(&self) -> Option<ProcessCpuSample> {
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        // SAFETY: self owns a live process handle and every FILETIME output is writable for the
        // call.
        let ok = unsafe {
            GetProcessTimes(
                self.0.raw(),
                &mut creation,
                &mut exit,
                &mut kernel,
                &mut user,
            )
        };
        if ok == 0 {
            None
        } else {
            Some(ProcessCpuSample {
                cpu_time_100ns: filetime_to_u64(kernel).saturating_add(filetime_to_u64(user)),
                sampled_at: Instant::now(),
            })
        }
    }

    fn creation_time_100ns(&self) -> Option<u64> {
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        // SAFETY: self owns a live process handle and every FILETIME output is writable for the
        // call.
        let ok = unsafe {
            GetProcessTimes(
                self.0.raw(),
                &mut creation,
                &mut exit,
                &mut kernel,
                &mut user,
            )
        };
        if ok == 0 {
            None
        } else {
            Some(filetime_to_u64(creation))
        }
    }
}
