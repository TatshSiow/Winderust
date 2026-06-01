use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::c_void,
    ptr::null_mut,
};

use windows_sys::Wdk::System::{
    SystemServices::PROCESS_EXTENDED_BASIC_INFORMATION,
    Threading::{
        NtQueryInformationProcess, NtQueryInformationThread, ProcessBasicInformation,
        ThreadSuspendCount,
    },
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, ERROR_ACCESS_DENIED, HANDLE, INVALID_HANDLE_VALUE},
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
        },
        RemoteDesktop::ProcessIdToSessionId,
        Threading::{
            GetCurrentProcessId, GetPriorityClass, GetProcessInformation, OpenProcess, OpenThread,
            ProcessPowerThrottling, SetPriorityClass, SetProcessInformation, IDLE_PRIORITY_CLASS,
            NORMAL_PRIORITY_CLASS, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE,
            PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION,
            PROCESS_SET_LIMITED_INFORMATION, THREAD_QUERY_INFORMATION,
            THREAD_QUERY_LIMITED_INFORMATION,
        },
    },
};

use crate::{config::EcoQosSettings, foreground::list_processes};

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
    "searchhost.exe",
    "securityhealthservice.exe",
    "securityhealthsystray.exe",
    "services.exe",
    "shellexperiencehost.exe",
    "sihost.exe",
    "smss.exe",
    "startmenuexperiencehost.exe",
    "system",
    "taskmgr.exe",
    "textinputhost.exe",
    "wininit.exe",
    "winlogon.exe",
    "wudfhost.exe",
];
const PROCESS_EXTENDED_BASIC_INFORMATION_IS_FROZEN: u32 = 0x10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EcoQosSnapshot {
    pub enabled: bool,
    pub scanned_processes: usize,
    pub throttled_processes: usize,
    pub skipped_processes: usize,
    pub failed_processes: usize,
    pub message: String,
    pub last_error: Option<String>,
}

#[derive(Default)]
pub struct EcoQosManager {
    throttled: BTreeMap<u32, ThrottledProcess>,
}

#[derive(Clone, Copy)]
struct ThrottledProcess {
    previous_state: Option<PROCESS_POWER_THROTTLING_STATE>,
    previous_priority: Option<u32>,
}

impl EcoQosManager {
    pub fn update(
        &mut self,
        settings: &EcoQosSettings,
        automation_enabled: bool,
        foreground_process_id: Option<u32>,
    ) -> EcoQosSnapshot {
        if !automation_enabled {
            let failed = self.clear_all();
            return EcoQosSnapshot {
                enabled: false,
                failed_processes: failed,
                message: "Automation disabled.".to_owned(),
                ..Default::default()
            };
        }

        if !settings.enabled {
            let failed = self.clear_all();
            return EcoQosSnapshot {
                enabled: false,
                failed_processes: failed,
                message: "Efficiency Mode disabled.".to_owned(),
                ..Default::default()
            };
        }

        if settings.exclude_foreground_app && foreground_process_id.is_none() {
            let failed = self.clear_all();
            return EcoQosSnapshot {
                enabled: true,
                failed_processes: failed,
                message: "Paused: foreground app is unknown.".to_owned(),
                ..Default::default()
            };
        }

        let current_process_id = unsafe { GetCurrentProcessId() };
        let Some(current_session_id) = process_session_id(current_process_id) else {
            let failed = self.clear_all();
            return EcoQosSnapshot {
                enabled: true,
                failed_processes: failed,
                message: "Paused: current Windows session is unknown.".to_owned(),
                ..Default::default()
            };
        };

        let processes = match list_processes() {
            Ok(processes) => processes,
            Err(err) => {
                let failed = self.clear_all();
                return EcoQosSnapshot {
                    enabled: true,
                    failed_processes: failed,
                    message: err,
                    ..Default::default()
                };
            }
        };

        let scanned_processes = processes.len();
        let mut skipped_processes = 0;
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
                || should_ignore_foreground_process(
                    settings,
                    process.id,
                    &process.name,
                    foreground_process_id,
                    foreground_process_name.as_deref(),
                )
                || is_process_excluded(&process.name, settings)
            {
                continue;
            }

            if process_session_id(process.id) != Some(current_session_id) {
                continue;
            }

            if settings.exclude_suspended_processes
                && process_is_windows_suspended(process.id).unwrap_or(false)
            {
                skipped_processes += 1;
                continue;
            }

            target_processes.insert(process.id, process.name);
        }

        let target_ids = target_processes.keys().copied().collect::<BTreeSet<_>>();
        let mut failed_processes = self.release_non_targets(&target_ids);
        let mut last_error = None;

        for (process_id, _name) in target_processes {
            if self.throttled.contains_key(&process_id) {
                continue;
            }

            match enable_efficiency_mode(process_id) {
                Ok(process) => {
                    self.throttled.insert(process_id, process);
                }
                Err(EcoQosError::AccessDenied) => {
                    skipped_processes += 1;
                }
                Err(EcoQosError::Failed(err)) => {
                    failed_processes += 1;
                    if last_error.is_none() {
                        last_error = Some(err);
                    }
                }
            }
        }

        EcoQosSnapshot {
            enabled: true,
            scanned_processes,
            throttled_processes: self.throttled.len(),
            skipped_processes,
            failed_processes,
            message: "Efficiency Mode active.".to_owned(),
            last_error,
        }
    }

    fn release_non_targets(&mut self, target_ids: &BTreeSet<u32>) -> usize {
        let process_ids = self
            .throttled
            .keys()
            .copied()
            .filter(|process_id| !target_ids.contains(process_id))
            .collect::<Vec<_>>();

        self.release_processes(&process_ids)
    }

    fn clear_all(&mut self) -> usize {
        let process_ids = self.throttled.keys().copied().collect::<Vec<_>>();
        self.release_processes(&process_ids)
    }

    fn release_processes(&mut self, process_ids: &[u32]) -> usize {
        let mut failed = 0;
        for process_id in process_ids {
            if let Some(process) = self.throttled.remove(process_id) {
                if restore_efficiency_mode(*process_id, process).is_err() {
                    failed += 1;
                }
            }
        }
        failed
    }
}

fn should_ignore_foreground_process(
    settings: &EcoQosSettings,
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

impl Drop for EcoQosManager {
    fn drop(&mut self) {
        self.clear_all();
    }
}

impl Default for EcoQosSnapshot {
    fn default() -> Self {
        Self {
            enabled: false,
            scanned_processes: 0,
            throttled_processes: 0,
            skipped_processes: 0,
            failed_processes: 0,
            message: "Efficiency Mode disabled.".to_owned(),
            last_error: None,
        }
    }
}

pub fn is_process_excluded(process_name: &str, settings: &EcoQosSettings) -> bool {
    let process_name = process_name.trim();
    BUILT_IN_EXCLUSIONS
        .iter()
        .any(|excluded| excluded.eq_ignore_ascii_case(process_name))
        || settings
            .efficiency_whitelist
            .iter()
            .any(|excluded| excluded.trim().eq_ignore_ascii_case(process_name))
}

fn process_session_id(process_id: u32) -> Option<u32> {
    let mut session_id = 0;
    let ok = unsafe { ProcessIdToSessionId(process_id, &mut session_id) };
    (ok != 0).then_some(session_id)
}

fn process_is_windows_suspended(process_id: u32) -> Result<bool, EcoQosError> {
    let frozen = ProcessHandle::open_query(process_id)
        .and_then(|process| process.windows_suspended())
        .unwrap_or(false);

    if frozen {
        return Ok(true);
    }

    process_threads_are_suspended(process_id)
}

fn process_frozen_from_flags(flags: u32) -> bool {
    flags & PROCESS_EXTENDED_BASIC_INFORMATION_IS_FROZEN != 0
}

fn process_threads_are_suspended(process_id: u32) -> Result<bool, EcoQosError> {
    let thread_ids = process_thread_ids(process_id)?;
    let mut suspend_counts = Vec::with_capacity(thread_ids.len());

    for thread_id in thread_ids {
        let thread = ThreadHandle::open_query(thread_id)?;
        suspend_counts.push(thread.suspend_count()?);
    }

    Ok(suspend_counts_indicate_suspended(&suspend_counts))
}

fn suspend_counts_indicate_suspended(suspend_counts: &[u32]) -> bool {
    !suspend_counts.is_empty() && suspend_counts.iter().all(|count| *count > 0)
}

fn process_thread_ids(process_id: u32) -> Result<Vec<u32>, EcoQosError> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(EcoQosError::Failed(format!(
            "Failed to read thread list with error {}.",
            last_error()
        )));
    }

    let mut entry = THREADENTRY32 {
        dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    let mut thread_ids = Vec::new();

    let mut has_entry = unsafe { Thread32First(snapshot, &mut entry) != 0 };
    while has_entry {
        if entry.th32OwnerProcessID == process_id {
            thread_ids.push(entry.th32ThreadID);
        }

        has_entry = unsafe { Thread32Next(snapshot, &mut entry) != 0 };
    }

    unsafe {
        CloseHandle(snapshot);
    }

    Ok(thread_ids)
}

enum EcoQosError {
    AccessDenied,
    Failed(String),
}

fn enable_efficiency_mode(process_id: u32) -> Result<ThrottledProcess, EcoQosError> {
    let process = ProcessHandle::open(process_id)?;
    let previous_state = process.power_throttling_state().ok();
    let previous_priority = process.priority_class().ok();

    let mut next_state = previous_state.unwrap_or_default();
    next_state.Version = PROCESS_POWER_THROTTLING_CURRENT_VERSION;
    next_state.ControlMask |= PROCESS_POWER_THROTTLING_EXECUTION_SPEED;
    next_state.StateMask |= PROCESS_POWER_THROTTLING_EXECUTION_SPEED;
    process.set_power_throttling_state(next_state)?;
    if let Err(err) = process.set_priority_class(IDLE_PRIORITY_CLASS) {
        let _ = process.set_power_throttling_state(
            previous_state.unwrap_or_else(power_throttling_disabled_state),
        );
        return Err(err);
    }

    Ok(ThrottledProcess {
        previous_state,
        previous_priority,
    })
}

fn restore_efficiency_mode(
    process_id: u32,
    process_state: ThrottledProcess,
) -> Result<(), EcoQosError> {
    let process = ProcessHandle::open(process_id)?;
    let mut last_error = None;

    if let Err(err) = process.set_power_throttling_state(
        process_state
            .previous_state
            .unwrap_or_else(power_throttling_disabled_state),
    ) {
        last_error = Some(err);
    }

    if let Err(err) = process.set_priority_class(
        process_state
            .previous_priority
            .unwrap_or(NORMAL_PRIORITY_CLASS),
    ) {
        last_error = Some(err);
    }

    match last_error {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

fn power_throttling_disabled_state() -> PROCESS_POWER_THROTTLING_STATE {
    PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        StateMask: 0,
    }
}

struct ProcessHandle(HANDLE);
struct ThreadHandle(HANDLE);

impl ProcessHandle {
    fn open_query(process_id: u32) -> Result<Self, EcoQosError> {
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
        if !handle.is_null() {
            return Ok(Self(handle));
        }

        let error = last_error();
        if error == ERROR_ACCESS_DENIED {
            Err(EcoQosError::AccessDenied)
        } else {
            Err(EcoQosError::Failed(format!(
                "OpenProcess({process_id}) failed with error {error}."
            )))
        }
    }

    fn open(process_id: u32) -> Result<Self, EcoQosError> {
        let access_masks = [
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_INFORMATION,
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_LIMITED_INFORMATION,
        ];

        let mut last_open_error = 0;
        for access in access_masks {
            let handle = unsafe { OpenProcess(access, 0, process_id) };
            if !handle.is_null() {
                return Ok(Self(handle));
            }
            last_open_error = last_error();
        }

        if last_open_error == ERROR_ACCESS_DENIED {
            Err(EcoQosError::AccessDenied)
        } else {
            Err(EcoQosError::Failed(format!(
                "OpenProcess({process_id}) failed with error {last_open_error}."
            )))
        }
    }

    fn power_throttling_state(&self) -> Result<PROCESS_POWER_THROTTLING_STATE, EcoQosError> {
        let mut state = PROCESS_POWER_THROTTLING_STATE::default();
        let ok = unsafe {
            GetProcessInformation(
                self.0,
                ProcessPowerThrottling,
                &mut state as *mut _ as *mut c_void,
                std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
            )
        };
        if ok == 0 {
            Err(EcoQosError::Failed(format!(
                "GetProcessInformation failed with error {}.",
                last_error()
            )))
        } else {
            Ok(state)
        }
    }

    fn windows_suspended(&self) -> Result<bool, EcoQosError> {
        let mut info = PROCESS_EXTENDED_BASIC_INFORMATION {
            Size: std::mem::size_of::<PROCESS_EXTENDED_BASIC_INFORMATION>(),
            ..Default::default()
        };
        let status = unsafe {
            NtQueryInformationProcess(
                self.0,
                ProcessBasicInformation,
                &mut info as *mut _ as *mut c_void,
                std::mem::size_of::<PROCESS_EXTENDED_BASIC_INFORMATION>() as u32,
                null_mut(),
            )
        };

        if status < 0 {
            return Err(EcoQosError::Failed(format!(
                "NtQueryInformationProcess failed with status 0x{:08X}.",
                status as u32
            )));
        }

        Ok(process_frozen_from_flags(unsafe { info.Anonymous.Flags }))
    }

    fn priority_class(&self) -> Result<u32, EcoQosError> {
        let priority = unsafe { GetPriorityClass(self.0) };
        if priority == 0 {
            Err(EcoQosError::Failed(format!(
                "GetPriorityClass failed with error {}.",
                last_error()
            )))
        } else {
            Ok(priority)
        }
    }

    fn set_power_throttling_state(
        &self,
        state: PROCESS_POWER_THROTTLING_STATE,
    ) -> Result<(), EcoQosError> {
        let ok = unsafe {
            SetProcessInformation(
                self.0,
                ProcessPowerThrottling,
                &state as *const _ as *const c_void,
                std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
            )
        };
        if ok == 0 {
            Err(EcoQosError::Failed(format!(
                "SetProcessInformation failed with error {}.",
                last_error()
            )))
        } else {
            Ok(())
        }
    }

    fn set_priority_class(&self, priority_class: u32) -> Result<(), EcoQosError> {
        let ok = unsafe { SetPriorityClass(self.0, priority_class) };
        if ok == 0 {
            Err(EcoQosError::Failed(format!(
                "SetPriorityClass failed with error {}.",
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

impl ThreadHandle {
    fn open_query(thread_id: u32) -> Result<Self, EcoQosError> {
        let access_masks = [THREAD_QUERY_INFORMATION, THREAD_QUERY_LIMITED_INFORMATION];

        let mut last_open_error = 0;
        for access in access_masks {
            let handle = unsafe { OpenThread(access, 0, thread_id) };
            if !handle.is_null() {
                return Ok(Self(handle));
            }
            last_open_error = last_error();
        }

        if last_open_error == ERROR_ACCESS_DENIED {
            Err(EcoQosError::AccessDenied)
        } else {
            Err(EcoQosError::Failed(format!(
                "OpenThread({thread_id}) failed with error {last_open_error}."
            )))
        }
    }

    fn suspend_count(&self) -> Result<u32, EcoQosError> {
        let mut suspend_count = 0u32;
        let status = unsafe {
            NtQueryInformationThread(
                self.0,
                ThreadSuspendCount,
                &mut suspend_count as *mut _ as *mut c_void,
                std::mem::size_of::<u32>() as u32,
                null_mut(),
            )
        };

        if status < 0 {
            return Err(EcoQosError::Failed(format!(
                "NtQueryInformationThread failed with status 0x{:08X}.",
                status as u32
            )));
        }

        Ok(suspend_count)
    }
}

impl Drop for ThreadHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn last_error() -> u32 {
    unsafe { GetLastError() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclusions_include_builtin_and_user_entries() {
        let settings = EcoQosSettings {
            enabled: true,
            exclude_foreground_app: true,
            exclude_suspended_processes: false,
            efficiency_whitelist: vec!["mouse.exe".to_owned()],
        };

        assert!(is_process_excluded("EXPLORER.EXE", &settings));
        assert!(is_process_excluded("csrss.exe", &settings));
        assert!(is_process_excluded("winlogon.exe", &settings));
        assert!(is_process_excluded("Mouse.exe", &settings));
        assert!(!is_process_excluded("browser.exe", &settings));
    }

    #[test]
    fn disabled_state_clears_execution_speed_control() {
        let state = power_throttling_disabled_state();

        assert_eq!(state.Version, PROCESS_POWER_THROTTLING_CURRENT_VERSION);
        assert_eq!(state.ControlMask, PROCESS_POWER_THROTTLING_EXECUTION_SPEED);
        assert_eq!(state.StateMask, 0);
    }

    #[test]
    fn frozen_process_flag_matches_windows_is_frozen_bit() {
        assert!(process_frozen_from_flags(
            PROCESS_EXTENDED_BASIC_INFORMATION_IS_FROZEN
        ));
        assert!(!process_frozen_from_flags(0));
    }

    #[test]
    fn suspend_counts_require_every_thread_to_be_suspended() {
        assert!(suspend_counts_indicate_suspended(&[1]));
        assert!(suspend_counts_indicate_suspended(&[2, 1]));
        assert!(!suspend_counts_indicate_suspended(&[]));
        assert!(!suspend_counts_indicate_suspended(&[1, 0]));
        assert!(!suspend_counts_indicate_suspended(&[0]));
    }

    #[test]
    fn foreground_ignore_matches_pid_or_process_name() {
        let mut settings = EcoQosSettings::default();
        settings.exclude_foreground_app = true;

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

        settings.exclude_foreground_app = false;
        assert!(!should_ignore_foreground_process(
            &settings,
            42,
            "app.exe",
            Some(42),
            Some("app.exe"),
        ));
    }
}
