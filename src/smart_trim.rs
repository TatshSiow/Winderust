use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::c_void,
    time::{Duration, Instant},
};

use windows_sys::Win32::{
    Foundation::{
        CloseHandle, GetLastError, ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, FILETIME, HANDLE,
    },
    System::{
        RemoteDesktop::ProcessIdToSessionId,
        SystemInformation::{GetSystemInfo, GlobalMemoryStatusEx, MEMORYSTATUSEX, SYSTEM_INFO},
        Threading::{
            GetCurrentProcessId, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_SET_QUOTA,
        },
    },
};

use crate::{
    action_log::{ActionLog, ActionLogAction, ActionLogFeature, ActionLogResult},
    config::SmartTrimSettings,
    foreground::list_processes,
    privilege::{enable_increase_quota_privilege, enable_profile_single_process_privilege},
    rules::{execution_failure_suppression_threshold, ExecutionFailureState},
};

const MB: u64 = 1024 * 1024;
const SYSTEM_MEMORY_LIST_INFORMATION_CLASS: u32 = 80;
const SYSTEM_FILE_CACHE_INFORMATION_EX_CLASS: u32 = 81;
const MEMORY_PURGE_STANDBY_LIST: u32 = 4;

const BUILT_IN_EXCLUSIONS: &[&str] = &[
    "audiodg.exe",
    "conhost.exe",
    "csrss.exe",
    "ctfmon.exe",
    "dwm.exe",
    "explorer.exe",
    "fontdrvhost.exe",
    "lsaiso.exe",
    "lsass.exe",
    "registry",
    "searchapp.exe",
    "searchhost.exe",
    "securityhealthservice.exe",
    "securityhealthsystray.exe",
    "services.exe",
    "shellexperiencehost.exe",
    "sihost.exe",
    "smss.exe",
    "startmenuexperiencehost.exe",
    "system",
    "systemsettings.exe",
    "taskmgr.exe",
    "textinputhost.exe",
    "wininit.exe",
    "winlogon.exe",
    "wudfhost.exe",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmartTrimSnapshot {
    pub enabled: bool,
    pub scanned_processes: usize,
    pub candidate_processes: usize,
    pub trimmed_processes: usize,
    pub skipped_processes: usize,
    pub failed_processes: usize,
    pub purged_standby_list: bool,
    pub purged_system_file_cache: bool,
    pub free_ram_excluding_cache_mb: Option<u64>,
    pub memory_load_percent: Option<u8>,
    pub trimmed_apps: Vec<String>,
    pub message: String,
    pub last_error: Option<String>,
}

#[derive(Default)]
pub struct SmartTrimManager {
    tracked: BTreeMap<u32, TrackedProcess>,
    last_trimmed: BTreeMap<u32, Instant>,
    failure_suppression: BTreeMap<String, SmartTrimFailureSuppression>,
    system_failure_suppression: BTreeMap<String, SmartTrimFailureSuppression>,
}

type SmartTrimFailureSuppression = ExecutionFailureState;

#[derive(Clone)]
struct TrackedProcess {
    process_name: String,
    previous_cpu_time: Option<ProcessCpuSample>,
    idle_since: Option<Instant>,
}

#[derive(Clone, Copy)]
struct ProcessCpuSample {
    cpu_time_100ns: u64,
    sampled_at: Instant,
}

#[derive(Clone, Copy)]
struct ProcessMemorySample {
    working_set_bytes: u64,
}

enum SmartTrimError {
    AccessDenied,
    ProcessExited,
    Failed(String),
}

impl SmartTrimManager {
    pub fn update(
        &mut self,
        settings: &SmartTrimSettings,
        automation_enabled: bool,
        foreground_process_id: Option<u32>,
        performance_mode_active: bool,
        action_log: &mut ActionLog,
    ) -> SmartTrimSnapshot {
        self.update_with_mode(
            settings,
            automation_enabled,
            foreground_process_id,
            performance_mode_active,
            SmartTrimMode::Automatic,
            action_log,
        )
    }

    pub fn trim_now(
        &mut self,
        settings: &SmartTrimSettings,
        automation_enabled: bool,
        foreground_process_id: Option<u32>,
        performance_mode_active: bool,
        action_log: &mut ActionLog,
    ) -> SmartTrimSnapshot {
        self.update_with_mode(
            settings,
            automation_enabled,
            foreground_process_id,
            performance_mode_active,
            SmartTrimMode::Manual,
            action_log,
        )
    }

    fn update_with_mode(
        &mut self,
        settings: &SmartTrimSettings,
        automation_enabled: bool,
        foreground_process_id: Option<u32>,
        performance_mode_active: bool,
        mode: SmartTrimMode,
        action_log: &mut ActionLog,
    ) -> SmartTrimSnapshot {
        if !automation_enabled {
            self.clear_tracking();
            self.clear_failure_suppression();
            return SmartTrimSnapshot {
                enabled: false,
                message: "Automation disabled.".to_owned(),
                ..Default::default()
            };
        }

        if !settings.enabled {
            self.clear_tracking();
            self.clear_failure_suppression();
            return SmartTrimSnapshot {
                enabled: false,
                message: "SmartTrim disabled.".to_owned(),
                ..Default::default()
            };
        }

        if settings.exclude_foreground_app && foreground_process_id.is_none() {
            self.clear_tracking();
            return SmartTrimSnapshot {
                enabled: true,
                message: "Paused: foreground app is unknown.".to_owned(),
                ..Default::default()
            };
        }

        let memory_load_percent = match system_memory_load_percent() {
            Ok(percent) => percent,
            Err(err) => {
                self.clear_tracking();
                return SmartTrimSnapshot {
                    enabled: true,
                    message: err.clone(),
                    last_error: Some(err),
                    ..Default::default()
                };
            }
        };

        let threshold = settings.system_memory_load_threshold_percent.min(100);
        if mode == SmartTrimMode::Automatic && memory_load_percent < threshold {
            self.clear_tracking();
            return SmartTrimSnapshot {
                enabled: true,
                memory_load_percent: Some(memory_load_percent),
                message: format!("SmartTrim waiting for system memory load >= {threshold}%."),
                ..Default::default()
            };
        }

        let current_process_id = unsafe { GetCurrentProcessId() };
        let Some(current_session_id) = process_session_id(current_process_id) else {
            self.clear_tracking();
            return SmartTrimSnapshot {
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
                return SmartTrimSnapshot {
                    enabled: true,
                    memory_load_percent: Some(memory_load_percent),
                    message: err,
                    ..Default::default()
                };
            }
        };

        let scanned_processes = processes.len();
        let foreground_process_name = if settings.exclude_foreground_app {
            foreground_process_id.and_then(|id| {
                processes
                    .iter()
                    .find(|process| process.id == id)
                    .map(|process| process.name.clone())
            })
        } else {
            None
        };

        let mut target_processes = BTreeMap::new();
        for process in processes {
            if process.id == 0
                || process.id == current_process_id
                || is_builtin_excluded(&process.name)
                || settings.exclusion_enabled_for(&process.name)
                || should_ignore_foreground_process(
                    settings,
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
        self.last_trimmed
            .retain(|process_id, _| target_ids.contains(process_id));
        let active_target_names = target_processes
            .values()
            .map(|name| process_failure_key(name))
            .collect::<BTreeSet<_>>();
        self.failure_suppression
            .retain(|process_name, _| active_target_names.contains(process_name));

        let mut candidate_processes = 0;
        let mut trimmed_processes = 0;
        let mut skipped_processes = 0;
        let mut failures = SmartTrimFailures::default();
        let mut trimmed_apps = BTreeSet::new();
        let now = Instant::now();

        for (process_id, process_name) in target_processes {
            if self.is_process_suppressed(process_id, &process_name, action_log) {
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
                        ActionLogFeature::SmartTrim,
                        Some(process_id),
                        process_name,
                        ActionLogAction::Apply,
                        ActionLogResult::Applied,
                        trim_reason(mode, freed_bytes),
                    );
                }
                Err(SmartTrimError::ProcessExited) => {
                    skipped_processes += 1;
                    self.tracked.remove(&process_id);
                    self.last_trimmed.remove(&process_id);
                }
                Err(SmartTrimError::AccessDenied) => {
                    skipped_processes += 1;
                    self.record_process_failure(&process_name);
                    action_log.record(
                        ActionLogFeature::SmartTrim,
                        Some(process_id),
                        process_name,
                        ActionLogAction::Skip,
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

        let mut purged_standby_list = false;
        let mut purged_system_file_cache = false;
        let free_ram_excluding_cache_mb = free_ram_excluding_cache_mb().ok();
        let purge_allowed = purge_allowed(
            settings,
            mode,
            performance_mode_active,
            free_ram_excluding_cache_mb,
        );

        if settings.purge_standby_list && purge_allowed {
            if !self.is_system_action_suppressed(
                "purge-standby-list",
                "Purge standby list",
                action_log,
            ) {
                match purge_standby_list() {
                    Ok(()) => {
                        purged_standby_list = true;
                        self.clear_system_action_failure("purge-standby-list");
                        action_log.record(
                            ActionLogFeature::SmartTrim,
                            None,
                            "System".to_owned(),
                            ActionLogAction::Apply,
                            ActionLogResult::Applied,
                            "Purged standby list.",
                        );
                    }
                    Err(err) => {
                        self.record_system_action_failure("purge-standby-list");
                        failures.record_system("Purge standby list", err, action_log);
                    }
                }
            }
        } else {
            self.clear_system_action_failure("purge-standby-list");
        }

        if settings.purge_system_file_cache && purge_allowed {
            if !self.is_system_action_suppressed(
                "purge-system-file-cache",
                "Purge system file cache",
                action_log,
            ) {
                match purge_system_file_cache() {
                    Ok(()) => {
                        purged_system_file_cache = true;
                        self.clear_system_action_failure("purge-system-file-cache");
                        action_log.record(
                            ActionLogFeature::SmartTrim,
                            None,
                            "System".to_owned(),
                            ActionLogAction::Apply,
                            ActionLogResult::Applied,
                            "Purged system file cache.",
                        );
                    }
                    Err(err) => {
                        self.record_system_action_failure("purge-system-file-cache");
                        failures.record_system("Purge system file cache", err, action_log);
                    }
                }
            }
        } else {
            self.clear_system_action_failure("purge-system-file-cache");
        }

        SmartTrimSnapshot {
            enabled: true,
            scanned_processes,
            candidate_processes,
            trimmed_processes,
            skipped_processes,
            failed_processes: failures.count,
            purged_standby_list,
            purged_system_file_cache,
            free_ram_excluding_cache_mb,
            memory_load_percent: Some(memory_load_percent),
            trimmed_apps: trimmed_apps.into_iter().collect(),
            message: match mode {
                SmartTrimMode::Automatic => "SmartTrim active.".to_owned(),
                SmartTrimMode::Manual => "Manual SmartTrim pass completed.".to_owned(),
            },
            last_error: failures.last_error,
        }
    }

    fn update_process(
        &mut self,
        process_id: u32,
        process_name: String,
        settings: &SmartTrimSettings,
        mode: SmartTrimMode,
        now: Instant,
    ) -> Result<ProcessUpdate, SmartTrimError> {
        let process = ProcessHandle::open(process_id)?;
        let memory = process.memory_sample()?;
        if !settings.trim_working_sets {
            self.tracked.remove(&process_id);
            return Ok(ProcessUpdate::Waiting);
        }

        let threshold_bytes = settings.process_working_set_threshold_mb.saturating_mul(MB);
        if memory.working_set_bytes < threshold_bytes {
            self.tracked.remove(&process_id);
            return Ok(ProcessUpdate::Waiting);
        }

        if mode == SmartTrimMode::Automatic
            && self
                .last_trimmed
                .get(&process_id)
                .is_some_and(|trimmed_at| {
                    now.duration_since(*trimmed_at)
                        < Duration::from_secs(settings.trim_cooldown_seconds)
                })
        {
            return Ok(ProcessUpdate::Candidate);
        }

        if mode == SmartTrimMode::Manual {
            let before = memory.working_set_bytes;
            process.empty_working_set()?;
            let after = process
                .memory_sample()
                .map(|sample| sample.working_set_bytes)
                .unwrap_or(0);
            self.last_trimmed.insert(process_id, now);
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
            });
        state.process_name = process_name;

        let usage = state
            .previous_cpu_time
            .and_then(|previous| process_cpu_usage_percent(previous, cpu_sample));
        state.previous_cpu_time = Some(cpu_sample);
        let Some(usage) = usage else {
            return Ok(ProcessUpdate::Candidate);
        };

        let idle_threshold = f32::from(settings.process_cpu_idle_threshold_percent.min(100));
        if usage <= idle_threshold {
            let idle_since = *state.idle_since.get_or_insert(now);
            if now.duration_since(idle_since) < Duration::from_secs(settings.process_idle_seconds) {
                return Ok(ProcessUpdate::Candidate);
            }
        } else {
            state.idle_since = None;
            return Ok(ProcessUpdate::Candidate);
        }

        let before = memory.working_set_bytes;
        process.empty_working_set()?;
        let after = process
            .memory_sample()
            .map(|sample| sample.working_set_bytes)
            .unwrap_or(0);
        self.last_trimmed.insert(process_id, now);
        state.idle_since = Some(now);
        Ok(ProcessUpdate::Trimmed {
            freed_bytes: before.saturating_sub(after),
        })
    }

    fn clear_tracking(&mut self) {
        self.tracked.clear();
        self.last_trimmed.clear();
    }

    fn clear_failure_suppression(&mut self) {
        self.failure_suppression.clear();
        self.system_failure_suppression.clear();
    }

    fn is_process_suppressed(
        &mut self,
        process_id: u32,
        process_name: &str,
        action_log: &mut ActionLog,
    ) -> bool {
        let Some(suppression) = self
            .failure_suppression
            .get_mut(&process_failure_key(process_name))
        else {
            return false;
        };
        if !suppression.is_suppressed() {
            return false;
        }

        if suppression.mark_suppression_logged() {
            action_log.record(
                ActionLogFeature::SmartTrim,
                Some(process_id),
                process_name.to_owned(),
                ActionLogAction::Skip,
                ActionLogResult::Skipped,
                format!(
                    "Stopped retrying SmartTrim after {} failed attempts.",
                    execution_failure_suppression_threshold(),
                ),
            );
        }

        true
    }

    fn record_process_failure(&mut self, process_name: &str) {
        let suppression = self
            .failure_suppression
            .entry(process_failure_key(process_name))
            .or_default();
        suppression.record_failure();
    }

    fn clear_process_failure(&mut self, process_name: &str) {
        self.failure_suppression
            .remove(&process_failure_key(process_name));
    }

    fn is_system_action_suppressed(
        &mut self,
        key: &str,
        label: &str,
        action_log: &mut ActionLog,
    ) -> bool {
        let Some(suppression) = self.system_failure_suppression.get_mut(key) else {
            return false;
        };
        if !suppression.is_suppressed() {
            return false;
        }

        if suppression.mark_suppression_logged() {
            action_log.record(
                ActionLogFeature::SmartTrim,
                None,
                "System".to_owned(),
                ActionLogAction::Skip,
                ActionLogResult::Skipped,
                format!(
                    "Stopped retrying SmartTrim {label} after {} failed attempts.",
                    execution_failure_suppression_threshold(),
                ),
            );
        }

        true
    }

    fn record_system_action_failure(&mut self, key: &str) {
        let suppression = self
            .system_failure_suppression
            .entry(key.to_owned())
            .or_default();
        suppression.record_failure();
    }

    fn clear_system_action_failure(&mut self, key: &str) {
        self.system_failure_suppression.remove(key);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SmartTrimMode {
    Automatic,
    Manual,
}

enum ProcessUpdate {
    Waiting,
    Candidate,
    Trimmed { freed_bytes: u64 },
}

fn trim_reason(mode: SmartTrimMode, freed_bytes: u64) -> String {
    match mode {
        SmartTrimMode::Automatic => format!(
            "Trimmed working set; estimated freed {}.",
            size_label(freed_bytes)
        ),
        SmartTrimMode::Manual => format!(
            "Manually trimmed working set; estimated freed {}.",
            size_label(freed_bytes)
        ),
    }
}

#[derive(Default)]
struct SmartTrimFailures {
    count: usize,
    last_error: Option<String>,
}

impl SmartTrimFailures {
    fn record(
        &mut self,
        process_id: u32,
        process_name: &str,
        error: SmartTrimError,
        action_log: &mut ActionLog,
    ) {
        let message = smart_trim_error_message(error);
        if is_process_exited_message(&message) {
            return;
        }
        self.count += 1;
        if self.last_error.is_none() {
            self.last_error = Some(format!("Trim {process_name} ({process_id}): {message}"));
        }
        action_log.record(
            ActionLogFeature::SmartTrim,
            Some(process_id),
            process_name.to_owned(),
            ActionLogAction::Fail,
            ActionLogResult::Failed,
            message,
        );
    }

    fn record_system(&mut self, operation: &str, message: String, action_log: &mut ActionLog) {
        self.count += 1;
        if self.last_error.is_none() {
            self.last_error = Some(format!("{operation}: {message}"));
        }
        action_log.record(
            ActionLogFeature::SmartTrim,
            None,
            "System".to_owned(),
            ActionLogAction::Fail,
            ActionLogResult::Failed,
            format!("{operation}: {message}"),
        );
    }
}

fn purge_allowed(
    settings: &SmartTrimSettings,
    mode: SmartTrimMode,
    performance_mode_active: bool,
    free_ram_excluding_cache_mb: Option<u64>,
) -> bool {
    if settings.purge_only_in_performance_mode && !performance_mode_active {
        return false;
    }

    if mode == SmartTrimMode::Manual {
        return true;
    }

    free_ram_excluding_cache_mb
        .is_some_and(|free_mb| free_mb < settings.purge_free_ram_threshold_mb)
}

fn purge_standby_list() -> Result<(), String> {
    if !enable_profile_single_process_privilege() {
        return Err("SeProfileSingleProcessPrivilege is not available.".to_owned());
    }

    let mut command = MEMORY_PURGE_STANDBY_LIST;
    nt_status_result(unsafe {
        NtSetSystemInformation(
            SYSTEM_MEMORY_LIST_INFORMATION_CLASS,
            (&mut command as *mut u32).cast::<c_void>(),
            std::mem::size_of::<u32>() as u32,
        )
    })
    .map_err(|status| format!("NtSetSystemInformation(SystemMemoryListInformation) failed with NTSTATUS 0x{status:08X}."))
}

fn purge_system_file_cache() -> Result<(), String> {
    if !enable_increase_quota_privilege() {
        return Err("SeIncreaseQuotaPrivilege is not available.".to_owned());
    }

    let mut cache_info = SystemFileCacheInformation {
        minimum_working_set: usize::MAX,
        maximum_working_set: usize::MAX,
        ..Default::default()
    };
    nt_status_result(unsafe {
        NtSetSystemInformation(
            SYSTEM_FILE_CACHE_INFORMATION_EX_CLASS,
            (&mut cache_info as *mut SystemFileCacheInformation).cast::<c_void>(),
            std::mem::size_of::<SystemFileCacheInformation>() as u32,
        )
    })
    .map_err(|status| format!("NtSetSystemInformation(SystemFileCacheInformationEx) failed with NTSTATUS 0x{status:08X}."))
}

fn free_ram_excluding_cache_mb() -> Result<u64, String> {
    let mut info = SystemMemoryListInformation::default();
    nt_status_result(unsafe {
        NtQuerySystemInformation(
            SYSTEM_MEMORY_LIST_INFORMATION_CLASS,
            (&mut info as *mut SystemMemoryListInformation).cast::<c_void>(),
            std::mem::size_of::<SystemMemoryListInformation>() as u32,
            std::ptr::null_mut(),
        )
    })
    .map_err(|status| format!("NtQuerySystemInformation(SystemMemoryListInformation) failed with NTSTATUS 0x{status:08X}."))?;

    Ok(
        (info.zero_page_count.saturating_add(info.free_page_count) as u64)
            .saturating_mul(u64::from(system_page_size()))
            / MB,
    )
}

fn nt_status_result(status: i32) -> Result<(), u32> {
    if status >= 0 {
        Ok(())
    } else {
        Err(status as u32)
    }
}

struct ProcessHandle(HANDLE);

impl ProcessHandle {
    fn open(process_id: u32) -> Result<Self, SmartTrimError> {
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_QUOTA,
                0,
                process_id,
            )
        };
        if !handle.is_null() {
            Ok(Self(handle))
        } else {
            Err(open_process_error(process_id, last_error()))
        }
    }

    fn memory_sample(&self) -> Result<ProcessMemorySample, SmartTrimError> {
        let mut counters = ProcessMemoryCounters {
            cb: std::mem::size_of::<ProcessMemoryCounters>() as u32,
            ..Default::default()
        };
        let ok = unsafe {
            K32GetProcessMemoryInfo(
                self.0,
                &mut counters,
                std::mem::size_of::<ProcessMemoryCounters>() as u32,
            )
        };
        if ok == 0 {
            Err(SmartTrimError::Failed(format!(
                "K32GetProcessMemoryInfo failed with error {}.",
                last_error()
            )))
        } else {
            Ok(ProcessMemorySample {
                working_set_bytes: counters.working_set_size as u64,
            })
        }
    }

    fn cpu_sample(&self) -> Result<ProcessCpuSample, SmartTrimError> {
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let ok =
            unsafe { GetProcessTimes(self.0, &mut creation, &mut exit, &mut kernel, &mut user) };
        if ok == 0 {
            Err(SmartTrimError::Failed(format!(
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

    fn empty_working_set(&self) -> Result<(), SmartTrimError> {
        let ok = unsafe { SetProcessWorkingSetSize(self.0, usize::MAX, usize::MAX) };
        if ok == 0 {
            Err(SmartTrimError::Failed(format!(
                "SetProcessWorkingSetSize failed with error {}.",
                last_error()
            )))
        } else {
            Ok(())
        }
    }
}

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn system_memory_load_percent() -> Result<u8, String> {
    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
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

fn process_cpu_usage_percent(previous: ProcessCpuSample, current: ProcessCpuSample) -> Option<f32> {
    let elapsed = current.sampled_at.duration_since(previous.sampled_at);
    let elapsed_100ns = elapsed.as_nanos() / 100;
    if elapsed_100ns == 0 {
        return None;
    }

    let cpu_delta = current
        .cpu_time_100ns
        .saturating_sub(previous.cpu_time_100ns) as f64;
    let processor_count = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .max(1) as f64;
    Some(((cpu_delta / (elapsed_100ns as f64 * processor_count)) * 100.0).clamp(0.0, 100.0) as f32)
}

fn should_ignore_foreground_process(
    settings: &SmartTrimSettings,
    process_id: u32,
    process_name: &str,
    foreground_process_id: Option<u32>,
    foreground_process_name: Option<&str>,
) -> bool {
    settings.exclude_foreground_app
        && (foreground_process_id.is_some_and(|id| id == process_id)
            || foreground_process_name
                .is_some_and(|name| name.eq_ignore_ascii_case(process_name.trim())))
}

pub fn is_builtin_excluded(process_name: &str) -> bool {
    BUILT_IN_EXCLUSIONS
        .iter()
        .any(|excluded| excluded.eq_ignore_ascii_case(process_name.trim()))
}

fn process_session_id(process_id: u32) -> Option<u32> {
    let mut session_id = 0;
    let ok = unsafe { ProcessIdToSessionId(process_id, &mut session_id) };
    (ok != 0).then_some(session_id)
}

fn open_process_error(process_id: u32, error: u32) -> SmartTrimError {
    match error {
        ERROR_ACCESS_DENIED => SmartTrimError::AccessDenied,
        ERROR_INVALID_PARAMETER => SmartTrimError::ProcessExited,
        _ => SmartTrimError::Failed(format!(
            "OpenProcess({process_id}) failed with error {error}."
        )),
    }
}

fn smart_trim_error_message(error: SmartTrimError) -> String {
    match error {
        SmartTrimError::AccessDenied => "Access denied.".to_owned(),
        SmartTrimError::ProcessExited => "Process exited.".to_owned(),
        SmartTrimError::Failed(message) => message,
    }
}

fn is_process_exited_message(message: &str) -> bool {
    message
        .trim()
        .trim_end_matches('.')
        .eq_ignore_ascii_case("Process exited")
}

fn process_failure_key(process_name: &str) -> String {
    process_name.trim().to_ascii_lowercase()
}

fn filetime_to_u64(value: FILETIME) -> u64 {
    (u64::from(value.dwHighDateTime) << 32) | u64::from(value.dwLowDateTime)
}

fn size_label(bytes: u64) -> String {
    if bytes >= MB {
        format!("{} MiB", bytes / MB)
    } else {
        format!("{} KiB", bytes / 1024)
    }
}

fn last_error() -> u32 {
    unsafe { GetLastError() }
}

fn system_page_size() -> u32 {
    let mut info = std::mem::MaybeUninit::<SYSTEM_INFO>::zeroed();
    unsafe {
        GetSystemInfo(info.as_mut_ptr());
        info.assume_init().dwPageSize
    }
}

#[repr(C)]
#[derive(Default)]
struct SystemMemoryListInformation {
    zero_page_count: usize,
    free_page_count: usize,
    modified_page_count: usize,
    modified_no_write_page_count: usize,
    bad_page_count: usize,
    page_count_by_priority: [usize; 8],
    repurposed_pages_by_priority: [usize; 8],
    modified_page_count_page_file: usize,
}

#[repr(C)]
#[derive(Default)]
struct SystemFileCacheInformation {
    current_size: usize,
    peak_size: usize,
    page_fault_count: u32,
    minimum_working_set: usize,
    maximum_working_set: usize,
    current_size_including_transition_in_pages: usize,
    peak_size_including_transition_in_pages: usize,
    transition_repurpose_count: u32,
    flags: u32,
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
    fn NtQuerySystemInformation(
        SystemInformationClass: u32,
        SystemInformation: *mut c_void,
        SystemInformationLength: u32,
        ReturnLength: *mut u32,
    ) -> i32;

    fn NtSetSystemInformation(
        SystemInformationClass: u32,
        SystemInformation: *mut c_void,
        SystemInformationLength: u32,
    ) -> i32;

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

impl Default for SmartTrimSnapshot {
    fn default() -> Self {
        Self {
            enabled: false,
            scanned_processes: 0,
            candidate_processes: 0,
            trimmed_processes: 0,
            skipped_processes: 0,
            failed_processes: 0,
            purged_standby_list: false,
            purged_system_file_cache: false,
            free_ram_excluding_cache_mb: None,
            memory_load_percent: None,
            trimmed_apps: Vec::new(),
            message: "SmartTrim disabled.".to_owned(),
            last_error: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_process_failures_suppress_smart_trim_retries() {
        let mut manager = SmartTrimManager::default();
        let mut log = ActionLog::new(8);

        manager.record_process_failure("APP.exe");
        manager.record_process_failure("app.exe");
        assert!(!manager.is_process_suppressed(42, "app.exe", &mut log));

        manager.record_process_failure("app.exe");
        assert!(manager.is_process_suppressed(42, "app.exe", &mut log));
        assert!(manager.is_process_suppressed(43, "APP.exe", &mut log));

        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].feature, ActionLogFeature::SmartTrim);
        assert_eq!(entries[0].action, ActionLogAction::Skip);
        assert_eq!(entries[0].result, ActionLogResult::Skipped);
        assert!(entries[0].reason.contains("Stopped retrying SmartTrim"));
    }

    #[test]
    fn successful_process_clears_smart_trim_failure_suppression() {
        let mut manager = SmartTrimManager::default();
        let mut log = ActionLog::new(8);

        manager.record_process_failure("app.exe");
        manager.record_process_failure("app.exe");
        manager.record_process_failure("app.exe");
        assert!(manager.is_process_suppressed(42, "app.exe", &mut log));

        manager.clear_process_failure("APP.exe");
        assert!(!manager.is_process_suppressed(42, "app.exe", &mut log));
    }

    #[test]
    fn repeated_system_failures_suppress_smart_trim_system_actions_once() {
        let mut manager = SmartTrimManager::default();
        let mut log = ActionLog::new(8);

        manager.record_system_action_failure("purge-standby-list");
        manager.record_system_action_failure("purge-standby-list");
        assert!(!manager.is_system_action_suppressed(
            "purge-standby-list",
            "Purge standby list",
            &mut log
        ));

        manager.record_system_action_failure("purge-standby-list");
        assert!(manager.is_system_action_suppressed(
            "purge-standby-list",
            "Purge standby list",
            &mut log
        ));
        assert!(manager.is_system_action_suppressed(
            "purge-standby-list",
            "Purge standby list",
            &mut log
        ));

        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].feature, ActionLogFeature::SmartTrim);
        assert_eq!(entries[0].action, ActionLogAction::Skip);
        assert_eq!(entries[0].result, ActionLogResult::Skipped);
        assert!(entries[0]
            .reason
            .contains("Stopped retrying SmartTrim Purge standby list"));
    }

    #[test]
    fn successful_system_action_clears_smart_trim_failure_suppression() {
        let mut manager = SmartTrimManager::default();
        let mut log = ActionLog::new(8);

        manager.record_system_action_failure("purge-standby-list");
        manager.record_system_action_failure("purge-standby-list");
        manager.record_system_action_failure("purge-standby-list");
        assert!(manager.is_system_action_suppressed(
            "purge-standby-list",
            "Purge standby list",
            &mut log
        ));

        manager.clear_system_action_failure("purge-standby-list");
        assert!(!manager.is_system_action_suppressed(
            "purge-standby-list",
            "Purge standby list",
            &mut log
        ));
    }

    #[test]
    fn builtin_exclusions_cover_sensitive_windows_processes() {
        assert!(is_builtin_excluded("csrss.exe"));
        assert!(is_builtin_excluded("winlogon.exe"));
        assert!(!is_builtin_excluded("worker.exe"));
    }

    #[test]
    fn foreground_skip_matches_pid_or_name() {
        let settings = SmartTrimSettings {
            enabled: true,
            check_interval_minutes: 15,
            exclude_foreground_app: true,
            trim_working_sets: true,
            system_memory_load_threshold_percent: 80,
            process_working_set_threshold_mb: 512,
            process_cpu_idle_threshold_percent: 1,
            process_idle_seconds: 300,
            trim_cooldown_seconds: 900,
            purge_standby_list: false,
            purge_system_file_cache: false,
            purge_only_in_performance_mode: true,
            purge_free_ram_threshold_mb: 1024,
            exclusions: Vec::new(),
        };

        assert!(should_ignore_foreground_process(
            &settings,
            42,
            "helper.exe",
            Some(42),
            Some("app.exe"),
        ));
        assert!(should_ignore_foreground_process(
            &settings,
            99,
            "APP.EXE",
            Some(42),
            Some("app.exe"),
        ));
        assert!(!should_ignore_foreground_process(
            &settings,
            99,
            "other.exe",
            Some(42),
            Some("app.exe"),
        ));
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
