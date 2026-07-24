use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

use windows_sys::Win32::{
    Foundation::{ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, FILETIME, HANDLE},
    System::{
        SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX},
        Threading::{
            GetCurrentProcessId, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_SET_QUOTA,
        },
    },
};

use crate::{
    action_log::{ActionLog, ActionLogFeature, ActionLogResult},
    config::MemoryTrimSettings,
    cpu::{process_cpu_usage_percent, ProcessCpuSample},
    foreground::{
        contains_process_name, list_processes, process_failure_key, process_session_id,
        should_ignore_foreground_process, EXTENDED_BUILT_IN_PROCESS_EXCLUSIONS,
    },
    rules::{execution_failure_suppression_threshold, ExecutionFailureTracker},
    win_util::{filetime_to_u64, last_error, WinHandle},
};

const MB: u64 = 1024 * 1024;
const CPU_IDLE_THRESHOLD_PERCENT: f32 = 1.0;

const BUILT_IN_EXCLUSIONS: &[&str] = EXTENDED_BUILT_IN_PROCESS_EXCLUSIONS;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryTrimSnapshot {
    pub enabled: bool,
    pub scanned_processes: usize,
    pub candidate_processes: usize,
    pub trimmed_processes: usize,
    pub skipped_processes: usize,
    pub failed_processes: usize,
    pub memory_load_percent: Option<u8>,
    pub trimmed_apps: Vec<String>,
    pub auto_excluded_processes: Vec<String>,
    pub message: String,
    pub last_error: Option<String>,
}

#[derive(Default)]
pub struct MemoryTrimManager {
    tracked: BTreeMap<u32, TrackedProcess>,
    failure_suppression: ExecutionFailureTracker,
}

#[derive(Clone)]
struct TrackedProcess {
    process_name: String,
    previous_cpu_time: Option<ProcessCpuSample>,
    idle_since: Option<Instant>,
    trimmed_while_idle: bool,
}

#[derive(Clone, Copy)]
struct ProcessMemorySample {
    working_set_bytes: u64,
}

enum MemoryTrimError {
    AccessDenied,
    ProcessExited,
    Failed(String),
}

impl MemoryTrimManager {
    pub fn update(
        &mut self,
        settings: &MemoryTrimSettings,
        automation_enabled: bool,
        foreground_process_id: Option<u32>,
        action_log: &mut ActionLog,
    ) -> MemoryTrimSnapshot {
        self.update_with_mode(
            settings,
            automation_enabled,
            foreground_process_id,
            MemoryTrimMode::Automatic,
            action_log,
        )
    }

    pub fn trim_now(
        &mut self,
        settings: &MemoryTrimSettings,
        automation_enabled: bool,
        foreground_process_id: Option<u32>,
        action_log: &mut ActionLog,
    ) -> MemoryTrimSnapshot {
        self.update_with_mode(
            settings,
            automation_enabled,
            foreground_process_id,
            MemoryTrimMode::Manual,
            action_log,
        )
    }

    fn update_with_mode(
        &mut self,
        settings: &MemoryTrimSettings,
        automation_enabled: bool,
        foreground_process_id: Option<u32>,
        mode: MemoryTrimMode,
        action_log: &mut ActionLog,
    ) -> MemoryTrimSnapshot {
        if !automation_enabled {
            self.clear_tracking();
            self.clear_failure_suppression();
            return MemoryTrimSnapshot {
                enabled: false,
                message: "Automation disabled.".to_owned(),
                ..Default::default()
            };
        }

        if !settings.enabled {
            self.clear_tracking();
            self.clear_failure_suppression();
            return MemoryTrimSnapshot {
                enabled: false,
                message: "Memory Trim disabled.".to_owned(),
                ..Default::default()
            };
        }

        if foreground_process_id.is_none() {
            self.clear_tracking();
            return MemoryTrimSnapshot {
                enabled: true,
                message: "Paused: foreground app is unknown.".to_owned(),
                ..Default::default()
            };
        }

        let memory_load_percent = match system_memory_load_percent() {
            Ok(percent) => percent,
            Err(err) => {
                self.clear_tracking();
                return MemoryTrimSnapshot {
                    enabled: true,
                    message: err.clone(),
                    last_error: Some(err),
                    ..Default::default()
                };
            }
        };

        let threshold = settings.system_memory_load_threshold_percent.min(100);
        if mode == MemoryTrimMode::Automatic && memory_load_percent < threshold {
            self.clear_tracking();
            return MemoryTrimSnapshot {
                enabled: true,
                memory_load_percent: Some(memory_load_percent),
                message: format!("Memory Trim waiting for system memory load >= {threshold}%."),
                ..Default::default()
            };
        }

        // SAFETY: GetCurrentProcessId takes no arguments and has no caller requirements.
        let current_process_id = unsafe { GetCurrentProcessId() };
        let Some(current_session_id) = process_session_id(current_process_id) else {
            self.clear_tracking();
            return MemoryTrimSnapshot {
                enabled: true,
                memory_load_percent: Some(memory_load_percent),
                message: "Paused: current Windows session is unknown.".to_owned(),
                ..Default::default()
            };
        };

        let processes = match list_processes() {
            Ok(processes) => processes,
            Err(err) => {
                self.clear_tracking();
                return MemoryTrimSnapshot {
                    enabled: true,
                    memory_load_percent: Some(memory_load_percent),
                    message: err,
                    ..Default::default()
                };
            }
        };

        let scanned_processes = processes.len();
        let foreground_process_name = foreground_process_id.and_then(|id| {
            processes
                .iter()
                .find(|process| process.id == id)
                .map(|process| process.name.clone())
        });

        let mut target_processes = BTreeMap::new();
        for process in processes {
            if process.id == 0
                || process.id == current_process_id
                || is_builtin_excluded(&process.name)
                || settings.exclusion_enabled_for(&process.name)
                || should_ignore_foreground_process(
                    true,
                    process.id,
                    &process.name,
                    foreground_process_id,
                    foreground_process_name.as_deref(),
                )
                || process_session_id(process.id) != Some(current_session_id)
            {
                continue;
            }

            target_processes.insert(process.id, process.name);
        }

        let target_ids = target_processes.keys().copied().collect::<BTreeSet<_>>();
        self.tracked
            .retain(|process_id, _| target_ids.contains(process_id));
        let active_target_names = target_processes
            .values()
            .map(|name| process_failure_key(name))
            .collect::<BTreeSet<_>>();
        self.failure_suppression.retain_keys(&active_target_names);

        let mut candidate_processes = 0;
        let mut trimmed_processes = 0;
        let mut skipped_processes = 0;
        let mut failures = MemoryTrimFailures::default();
        let mut trimmed_apps = BTreeSet::new();
        let mut auto_excluded_processes = BTreeSet::new();
        let now = Instant::now();

        for (process_id, process_name) in target_processes {
            if self.is_process_suppressed(
                process_id,
                &process_name,
                action_log,
                &mut auto_excluded_processes,
            ) {
                skipped_processes += 1;
                continue;
            }

            match self.update_process(process_id, process_name.clone(), settings, mode, now) {
                Ok(ProcessUpdate::Waiting) => {
                    self.clear_process_failure(&process_name);
                }
                Ok(ProcessUpdate::Candidate) => {
                    candidate_processes += 1;
                    self.clear_process_failure(&process_name);
                }
                Ok(ProcessUpdate::Trimmed { freed_bytes }) => {
                    candidate_processes += 1;
                    trimmed_processes += 1;
                    trimmed_apps.insert(process_name.clone());
                    self.clear_process_failure(&process_name);
                    action_log.record(
                        ActionLogFeature::MemoryTrim,
                        Some(process_id),
                        process_name,
                        ActionLogResult::Applied,
                        trim_reason(mode, freed_bytes),
                    );
                }
                Err(MemoryTrimError::ProcessExited) => {
                    skipped_processes += 1;
                    self.tracked.remove(&process_id);
                }
                Err(MemoryTrimError::AccessDenied) => {
                    skipped_processes += 1;
                    self.record_process_failure(&process_name);
                    action_log.record(
                        ActionLogFeature::MemoryTrim,
                        Some(process_id),
                        process_name,
                        ActionLogResult::Skipped,
                        "Skipped because the process could not be opened.",
                    );
                }
                Err(err) => {
                    self.record_process_failure(&process_name);
                    failures.record(process_id, &process_name, err, action_log);
                }
            }
        }

        MemoryTrimSnapshot {
            enabled: true,
            scanned_processes,
            candidate_processes,
            trimmed_processes,
            skipped_processes,
            failed_processes: failures.count,
            memory_load_percent: Some(memory_load_percent),
            trimmed_apps: trimmed_apps.into_iter().collect(),
            auto_excluded_processes: auto_excluded_processes.into_iter().collect(),
            message: match mode {
                MemoryTrimMode::Automatic => "Memory Trim active.".to_owned(),
                MemoryTrimMode::Manual => "Manual Memory Trim pass completed.".to_owned(),
            },
            last_error: failures.last_error,
        }
    }

    fn update_process(
        &mut self,
        process_id: u32,
        process_name: String,
        settings: &MemoryTrimSettings,
        mode: MemoryTrimMode,
        now: Instant,
    ) -> Result<ProcessUpdate, MemoryTrimError> {
        let process = ProcessHandle::open(process_id)?;
        let memory = process.memory_sample()?;
        let threshold_bytes = settings.process_working_set_threshold_mb.saturating_mul(MB);
        if memory.working_set_bytes < threshold_bytes {
            self.tracked.remove(&process_id);
            return Ok(ProcessUpdate::Waiting);
        }

        if mode == MemoryTrimMode::Manual {
            let before = memory.working_set_bytes;
            process.empty_working_set()?;
            let after = process
                .memory_sample()
                .map(|sample| sample.working_set_bytes)
                .unwrap_or(0);
            return Ok(ProcessUpdate::Trimmed {
                freed_bytes: before.saturating_sub(after),
            });
        }

        let cpu_sample = process.cpu_sample()?;
        let state = self
            .tracked
            .entry(process_id)
            .or_insert_with(|| TrackedProcess {
                process_name: process_name.clone(),
                previous_cpu_time: None,
                idle_since: None,
                trimmed_while_idle: false,
            });
        state.process_name = process_name;

        let usage = state
            .previous_cpu_time
            .and_then(|previous| process_cpu_usage_percent(previous, cpu_sample));
        state.previous_cpu_time = Some(cpu_sample);
        let Some(usage) = usage else {
            return Ok(ProcessUpdate::Candidate);
        };

        if !ready_to_trim(
            state,
            usage,
            Duration::from_secs(settings.process_idle_seconds),
            now,
        ) {
            return Ok(ProcessUpdate::Candidate);
        }

        let before = memory.working_set_bytes;
        process.empty_working_set()?;
        let after = process
            .memory_sample()
            .map(|sample| sample.working_set_bytes)
            .unwrap_or(0);
        state.trimmed_while_idle = true;
        Ok(ProcessUpdate::Trimmed {
            freed_bytes: before.saturating_sub(after),
        })
    }

    fn clear_tracking(&mut self) {
        self.tracked.clear();
    }

    fn clear_failure_suppression(&mut self) {
        self.failure_suppression.clear();
    }

    fn is_process_suppressed(
        &mut self,
        process_id: u32,
        process_name: &str,
        action_log: &mut ActionLog,
        auto_excluded_processes: &mut BTreeSet<String>,
    ) -> bool {
        let suppression = self.failure_suppression.process_suppression(process_name);
        if !suppression.suppressed {
            return false;
        }

        if suppression.newly_suppressed {
            auto_excluded_processes.insert(process_failure_key(process_name));
            action_log.record(
                ActionLogFeature::MemoryTrim,
                Some(process_id),
                process_name.to_owned(),
                ActionLogResult::Skipped,
                format!(
                    "Stopped retrying Memory Trim after {} failed attempts.",
                    execution_failure_suppression_threshold(),
                ),
            );
        }

        true
    }

    fn record_process_failure(&mut self, process_name: &str) {
        self.failure_suppression
            .record_process_failure(process_name);
    }

    fn clear_process_failure(&mut self, process_name: &str) {
        self.failure_suppression.clear_process_failure(process_name);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MemoryTrimMode {
    Automatic,
    Manual,
}

enum ProcessUpdate {
    Waiting,
    Candidate,
    Trimmed { freed_bytes: u64 },
}

fn ready_to_trim(
    state: &mut TrackedProcess,
    cpu_usage_percent: f32,
    idle_duration: Duration,
    now: Instant,
) -> bool {
    if cpu_usage_percent > CPU_IDLE_THRESHOLD_PERCENT {
        state.idle_since = None;
        state.trimmed_while_idle = false;
        return false;
    }
    if state.trimmed_while_idle {
        return false;
    }

    let idle_since = *state.idle_since.get_or_insert(now);
    now.duration_since(idle_since) >= idle_duration
}
fn trim_reason(mode: MemoryTrimMode, freed_bytes: u64) -> String {
    match mode {
        MemoryTrimMode::Automatic => format!(
            "Trimmed working set; estimated freed {}.",
            size_label(freed_bytes)
        ),
        MemoryTrimMode::Manual => format!(
            "Manually trimmed working set; estimated freed {}.",
            size_label(freed_bytes)
        ),
    }
}

#[derive(Default)]
struct MemoryTrimFailures {
    count: usize,
    last_error: Option<String>,
}

impl MemoryTrimFailures {
    fn record(
        &mut self,
        process_id: u32,
        process_name: &str,
        error: MemoryTrimError,
        action_log: &mut ActionLog,
    ) {
        if matches!(&error, MemoryTrimError::ProcessExited) {
            return;
        }
        let message = memory_trim_error_message(error);
        self.count += 1;
        if self.last_error.is_none() {
            self.last_error = Some(format!("Trim {process_name} ({process_id}): {message}"));
        }
        action_log.record(
            ActionLogFeature::MemoryTrim,
            Some(process_id),
            process_name.to_owned(),
            ActionLogResult::Failed,
            message,
        );
    }
}

struct ProcessHandle(WinHandle);

impl ProcessHandle {
    fn open(process_id: u32) -> Result<Self, MemoryTrimError> {
        // SAFETY: process_id came from the current process snapshot and no inherited handle is
        // requested.
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_QUOTA,
                0,
                process_id,
            )
        };
        if !handle.is_null() {
            Ok(Self(WinHandle::new(handle)))
        } else {
            Err(open_process_error(process_id, last_error()))
        }
    }

    fn memory_sample(&self) -> Result<ProcessMemorySample, MemoryTrimError> {
        let mut counters = ProcessMemoryCounters {
            cb: std::mem::size_of::<ProcessMemoryCounters>() as u32,
            ..Default::default()
        };
        // SAFETY: self owns a live process handle and counters is writable for exactly the
        // supplied structure size.
        let ok = unsafe {
            K32GetProcessMemoryInfo(
                self.0.raw(),
                &mut counters,
                std::mem::size_of::<ProcessMemoryCounters>() as u32,
            )
        };
        if ok == 0 {
            Err(MemoryTrimError::Failed(format!(
                "K32GetProcessMemoryInfo failed with error {}.",
                last_error()
            )))
        } else {
            Ok(ProcessMemorySample {
                working_set_bytes: counters.working_set_size as u64,
            })
        }
    }

    fn cpu_sample(&self) -> Result<ProcessCpuSample, MemoryTrimError> {
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
            Err(MemoryTrimError::Failed(format!(
                "GetProcessTimes failed with error {}.",
                last_error()
            )))
        } else {
            Ok(ProcessCpuSample {
                cpu_time_100ns: filetime_to_u64(kernel).saturating_add(filetime_to_u64(user)),
                sampled_at: Instant::now(),
            })
        }
    }

    fn empty_working_set(&self) -> Result<(), MemoryTrimError> {
        // SAFETY: self owns a live process handle; both usize::MAX values are the documented
        // request to empty the working set.
        let ok = unsafe { SetProcessWorkingSetSize(self.0.raw(), usize::MAX, usize::MAX) };
        if ok == 0 {
            Err(MemoryTrimError::Failed(format!(
                "SetProcessWorkingSetSize failed with error {}.",
                last_error()
            )))
        } else {
            Ok(())
        }
    }
}

fn system_memory_load_percent() -> Result<u8, String> {
    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: status declares its initialized size and remains writable for the call.
    let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
    if ok == 0 {
        Err(format!(
            "GlobalMemoryStatusEx failed with error {}.",
            last_error()
        ))
    } else {
        Ok(status.dwMemoryLoad.min(100) as u8)
    }
}

pub fn is_builtin_excluded(process_name: &str) -> bool {
    contains_process_name(BUILT_IN_EXCLUSIONS, process_name)
}

fn open_process_error(process_id: u32, error: u32) -> MemoryTrimError {
    match error {
        ERROR_ACCESS_DENIED => MemoryTrimError::AccessDenied,
        ERROR_INVALID_PARAMETER => MemoryTrimError::ProcessExited,
        _ => MemoryTrimError::Failed(format!(
            "OpenProcess({process_id}) failed with error {error}."
        )),
    }
}

fn memory_trim_error_message(error: MemoryTrimError) -> String {
    match error {
        MemoryTrimError::AccessDenied => "Access denied.".to_owned(),
        MemoryTrimError::ProcessExited => "Process exited.".to_owned(),
        MemoryTrimError::Failed(message) => message,
    }
}

fn size_label(bytes: u64) -> String {
    if bytes >= MB {
        format!("{} MiB", bytes / MB)
    } else {
        format!("{} KiB", bytes / 1024)
    }
}

#[repr(C)]
#[derive(Default)]
struct ProcessMemoryCounters {
    cb: u32,
    page_fault_count: u32,
    peak_working_set_size: usize,
    working_set_size: usize,
    quota_peak_paged_pool_usage: usize,
    quota_paged_pool_usage: usize,
    quota_peak_non_paged_pool_usage: usize,
    quota_non_paged_pool_usage: usize,
    pagefile_usage: usize,
    peak_pagefile_usage: usize,
}

unsafe extern "system" {
    fn K32GetProcessMemoryInfo(
        Process: HANDLE,
        Counters: *mut ProcessMemoryCounters,
        Size: u32,
    ) -> i32;

    fn SetProcessWorkingSetSize(
        hProcess: HANDLE,
        dwMinimumWorkingSetSize: usize,
        dwMaximumWorkingSetSize: usize,
    ) -> i32;
}

impl Default for MemoryTrimSnapshot {
    fn default() -> Self {
        Self {
            enabled: false,
            scanned_processes: 0,
            candidate_processes: 0,
            trimmed_processes: 0,
            skipped_processes: 0,
            failed_processes: 0,
            memory_load_percent: None,
            trimmed_apps: Vec::new(),
            auto_excluded_processes: Vec::new(),
            message: "Memory Trim disabled.".to_owned(),
            last_error: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_process_failures_suppress_memory_trim_retries() {
        let mut manager = MemoryTrimManager::default();
        let mut log = ActionLog::new(8);

        manager.record_process_failure("APP.exe");
        manager.record_process_failure("app.exe");
        assert!(!manager.is_process_suppressed(42, "app.exe", &mut log, &mut BTreeSet::new()));

        manager.record_process_failure("app.exe");
        assert!(manager.is_process_suppressed(42, "app.exe", &mut log, &mut BTreeSet::new()));
        assert!(manager.is_process_suppressed(43, "APP.exe", &mut log, &mut BTreeSet::new()));

        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].feature, ActionLogFeature::MemoryTrim);
        assert_eq!(entries[0].result, ActionLogResult::Skipped);
        assert!(entries[0].reason.contains("Stopped retrying Memory Trim"));
    }

    #[test]
    fn successful_process_clears_memory_trim_failure_suppression() {
        let mut manager = MemoryTrimManager::default();
        let mut log = ActionLog::new(8);

        manager.record_process_failure("app.exe");
        manager.record_process_failure("app.exe");
        manager.record_process_failure("app.exe");
        assert!(manager.is_process_suppressed(42, "app.exe", &mut log, &mut BTreeSet::new()));

        manager.clear_process_failure("APP.exe");
        assert!(!manager.is_process_suppressed(42, "app.exe", &mut log, &mut BTreeSet::new()));
    }

    #[test]
    fn builtin_exclusions_cover_sensitive_windows_processes() {
        assert!(is_builtin_excluded("csrss.exe"));
        assert!(is_builtin_excluded("winlogon.exe"));
        assert!(!is_builtin_excluded("worker.exe"));
    }

    #[test]
    fn foreground_skip_matches_pid_or_name() {
        assert!(should_ignore_foreground_process(
            true,
            42,
            "helper.exe",
            Some(42),
            Some("app.exe"),
        ));
        assert!(should_ignore_foreground_process(
            true,
            99,
            "APP.EXE",
            Some(42),
            Some("app.exe"),
        ));
        assert!(!should_ignore_foreground_process(
            true,
            99,
            "other.exe",
            Some(42),
            Some("app.exe"),
        ));
    }

    #[test]
    fn trim_eligibility_rearms_only_after_process_activity() {
        let now = Instant::now();
        let mut process = TrackedProcess {
            process_name: "app.exe".to_owned(),
            previous_cpu_time: None,
            idle_since: None,
            trimmed_while_idle: false,
        };
        let idle_duration = Duration::from_secs(300);

        assert!(!ready_to_trim(&mut process, 0.0, idle_duration, now));
        assert!(ready_to_trim(
            &mut process,
            0.0,
            idle_duration,
            now + idle_duration
        ));

        process.trimmed_while_idle = true;
        assert!(!ready_to_trim(
            &mut process,
            0.0,
            idle_duration,
            now + idle_duration
        ));
        assert!(!ready_to_trim(
            &mut process,
            2.0,
            idle_duration,
            now + idle_duration
        ));
        assert!(!process.trimmed_while_idle);
    }
    #[test]
    fn process_cpu_usage_percent_scales_by_processor_count() {
        let now = Instant::now();
        let previous = ProcessCpuSample {
            cpu_time_100ns: 0,
            sampled_at: now,
        };
        let current = ProcessCpuSample {
            cpu_time_100ns: 10_000_000,
            sampled_at: now + Duration::from_secs(1),
        };

        let usage = process_cpu_usage_percent(previous, current).unwrap();

        assert!(usage > 0.0);
        assert!(usage <= 100.0);
    }
}
