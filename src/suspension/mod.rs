use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, ERROR_ACCESS_DENIED, HANDLE, INVALID_HANDLE_VALUE},
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
        },
        RemoteDesktop::ProcessIdToSessionId,
        Threading::{
            GetCurrentProcessId, OpenThread, ResumeThread, SuspendThread, THREAD_SUSPEND_RESUME,
        },
    },
};

use crate::{config::AppSuspensionSettings, foreground::list_processes};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppSuspensionSnapshot {
    pub enabled: bool,
    pub tracked_processes: usize,
    pub suspended_processes: usize,
    pub skipped_processes: usize,
    pub failed_actions: usize,
    pub message: String,
    pub last_error: Option<String>,
}

#[derive(Default)]
pub struct AppSuspensionManager {
    tracked: BTreeMap<u32, TrackedProcess>,
    suspended: BTreeMap<u32, SuspendedProcess>,
}

struct TrackedProcess {
    background_since: Instant,
}

struct SuspendedProcess {
    suspended_threads: Vec<u32>,
}

impl AppSuspensionManager {
    pub fn update(
        &mut self,
        settings: &AppSuspensionSettings,
        automation_enabled: bool,
        foreground_process_id: Option<u32>,
    ) -> AppSuspensionSnapshot {
        if !automation_enabled {
            let failed = self.clear_all();
            return AppSuspensionSnapshot {
                enabled: false,
                failed_actions: failed,
                message: "Automation disabled.".to_owned(),
                ..Default::default()
            };
        }

        if !settings.enabled {
            let failed = self.clear_all();
            return AppSuspensionSnapshot {
                enabled: false,
                failed_actions: failed,
                message: "App Suspension disabled.".to_owned(),
                ..Default::default()
            };
        }

        let Some(foreground_process_id) = foreground_process_id else {
            let failed = self.clear_all();
            return AppSuspensionSnapshot {
                enabled: true,
                failed_actions: failed,
                message: "Paused: foreground app is unknown.".to_owned(),
                ..Default::default()
            };
        };

        let current_process_id = unsafe { GetCurrentProcessId() };
        let Some(current_session_id) = process_session_id(current_process_id) else {
            let failed = self.clear_all();
            return AppSuspensionSnapshot {
                enabled: true,
                failed_actions: failed,
                message: "Paused: current Windows session is unknown.".to_owned(),
                ..Default::default()
            };
        };

        let processes = match list_processes() {
            Ok(processes) => processes,
            Err(err) => {
                let failed = self.clear_all();
                return AppSuspensionSnapshot {
                    enabled: true,
                    failed_actions: failed,
                    message: err,
                    ..Default::default()
                };
            }
        };

        let foreground_process_name = processes
            .iter()
            .find(|process| process.id == foreground_process_id)
            .map(|process| process.name.clone());
        let delay = Duration::from_secs(settings.background_delay_seconds);
        let mut target_ids = BTreeSet::new();

        for process in processes {
            if process.id == 0
                || process.id == current_process_id
                || is_builtin_excluded(&process.name)
                || !contains_process(&settings.suspendable_apps, &process.name)
                || should_skip_foreground_process(
                    process.id,
                    &process.name,
                    foreground_process_id,
                    foreground_process_name.as_deref(),
                )
            {
                continue;
            }

            if process_session_id(process.id) != Some(current_session_id) {
                continue;
            }

            target_ids.insert(process.id);
        }

        let mut failed_actions = self.release_non_targets(&target_ids);
        self.tracked
            .retain(|process_id, _process| target_ids.contains(process_id));

        let mut skipped_processes = 0;
        let mut last_error = None;
        let now = Instant::now();

        for process_id in target_ids {
            if self.suspended.contains_key(&process_id) {
                continue;
            }

            let tracked = self
                .tracked
                .entry(process_id)
                .or_insert_with(|| TrackedProcess {
                    background_since: now,
                });
            if tracked.background_since.elapsed() < delay {
                continue;
            }

            match suspend_process(process_id) {
                Ok(suspended_process) => {
                    self.tracked.remove(&process_id);
                    self.suspended.insert(process_id, suspended_process);
                }
                Err(SuspensionError::AccessDenied) => {
                    skipped_processes += 1;
                }
                Err(SuspensionError::Failed(err)) => {
                    failed_actions += 1;
                    if last_error.is_none() {
                        last_error = Some(err);
                    }
                }
            }
        }

        AppSuspensionSnapshot {
            enabled: true,
            tracked_processes: self.tracked.len(),
            suspended_processes: self.suspended.len(),
            skipped_processes,
            failed_actions,
            message: "App Suspension active.".to_owned(),
            last_error,
        }
    }

    fn release_non_targets(&mut self, target_ids: &BTreeSet<u32>) -> usize {
        let process_ids = self
            .suspended
            .keys()
            .copied()
            .filter(|process_id| !target_ids.contains(process_id))
            .collect::<Vec<_>>();

        self.release_processes(&process_ids)
    }

    fn clear_all(&mut self) -> usize {
        self.tracked.clear();
        let process_ids = self.suspended.keys().copied().collect::<Vec<_>>();
        self.release_processes(&process_ids)
    }

    fn release_processes(&mut self, process_ids: &[u32]) -> usize {
        let mut failed = 0;
        for process_id in process_ids {
            if let Some(process) = self.suspended.remove(process_id) {
                if resume_process(process).is_err() {
                    failed += 1;
                }
            }
        }
        failed
    }
}

impl Drop for AppSuspensionManager {
    fn drop(&mut self) {
        self.clear_all();
    }
}

impl Default for AppSuspensionSnapshot {
    fn default() -> Self {
        Self {
            enabled: false,
            tracked_processes: 0,
            suspended_processes: 0,
            skipped_processes: 0,
            failed_actions: 0,
            message: "App Suspension disabled.".to_owned(),
            last_error: None,
        }
    }
}

pub fn is_builtin_excluded(process_name: &str) -> bool {
    BUILT_IN_EXCLUSIONS
        .iter()
        .any(|excluded| excluded.eq_ignore_ascii_case(process_name.trim()))
}

pub fn contains_process(list: &[String], process_name: &str) -> bool {
    list.iter()
        .any(|process| process.trim().eq_ignore_ascii_case(process_name.trim()))
}

fn should_skip_foreground_process(
    process_id: u32,
    process_name: &str,
    foreground_process_id: u32,
    foreground_process_name: Option<&str>,
) -> bool {
    process_id == foreground_process_id
        || foreground_process_name
            .is_some_and(|name| name.eq_ignore_ascii_case(process_name.trim()))
}

fn process_session_id(process_id: u32) -> Option<u32> {
    let mut session_id = 0;
    let ok = unsafe { ProcessIdToSessionId(process_id, &mut session_id) };
    (ok != 0).then_some(session_id)
}

enum SuspensionError {
    AccessDenied,
    Failed(String),
}

fn suspend_process(process_id: u32) -> Result<SuspendedProcess, SuspensionError> {
    let thread_ids = process_thread_ids(process_id)?;
    if thread_ids.is_empty() {
        return Err(SuspensionError::Failed(format!(
            "Process {process_id} has no suspendable threads."
        )));
    }

    let mut suspended_threads = Vec::new();
    for thread_id in thread_ids {
        match ThreadHandle::open(thread_id) {
            Ok(thread) => match thread.suspend() {
                Ok(()) => suspended_threads.push(thread_id),
                Err(err) => {
                    resume_threads(&suspended_threads);
                    return Err(err);
                }
            },
            Err(SuspensionError::AccessDenied) => {
                resume_threads(&suspended_threads);
                return Err(SuspensionError::AccessDenied);
            }
            Err(err) => {
                resume_threads(&suspended_threads);
                return Err(err);
            }
        }
    }

    Ok(SuspendedProcess { suspended_threads })
}

fn resume_process(process: SuspendedProcess) -> Result<(), SuspensionError> {
    let mut last_error = None;
    for thread_id in process.suspended_threads {
        match ThreadHandle::open(thread_id).and_then(|thread| thread.resume()) {
            Ok(()) | Err(SuspensionError::AccessDenied) => {}
            Err(err) => last_error = Some(err),
        }
    }

    match last_error {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

fn resume_threads(thread_ids: &[u32]) {
    for thread_id in thread_ids {
        if let Ok(thread) = ThreadHandle::open(*thread_id) {
            let _ = thread.resume();
        }
    }
}

fn process_thread_ids(process_id: u32) -> Result<Vec<u32>, SuspensionError> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(SuspensionError::Failed(format!(
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

struct ThreadHandle(HANDLE);

impl ThreadHandle {
    fn open(thread_id: u32) -> Result<Self, SuspensionError> {
        let handle = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, thread_id) };
        if handle.is_null() {
            let error = last_error();
            if error == ERROR_ACCESS_DENIED {
                Err(SuspensionError::AccessDenied)
            } else {
                Err(SuspensionError::Failed(format!(
                    "OpenThread({thread_id}) failed with error {error}."
                )))
            }
        } else {
            Ok(Self(handle))
        }
    }

    fn suspend(&self) -> Result<(), SuspensionError> {
        let previous_count = unsafe { SuspendThread(self.0) };
        if previous_count == u32::MAX {
            Err(SuspensionError::Failed(format!(
                "SuspendThread failed with error {}.",
                last_error()
            )))
        } else {
            Ok(())
        }
    }

    fn resume(&self) -> Result<(), SuspensionError> {
        let previous_count = unsafe { ResumeThread(self.0) };
        if previous_count == u32::MAX {
            Err(SuspensionError::Failed(format!(
                "ResumeThread failed with error {}.",
                last_error()
            )))
        } else {
            Ok(())
        }
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
    fn suspendable_app_match_is_case_insensitive() {
        let suspendable_apps = vec!["chat.exe".to_owned()];

        assert!(contains_process(&suspendable_apps, "CHAT.EXE"));
        assert!(!contains_process(&suspendable_apps, "browser.exe"));
    }

    #[test]
    fn foreground_skip_matches_pid_or_name() {
        assert!(should_skip_foreground_process(
            42,
            "helper.exe",
            42,
            Some("app.exe"),
        ));
        assert!(should_skip_foreground_process(
            99,
            "APP.EXE",
            42,
            Some("app.exe"),
        ));
        assert!(!should_skip_foreground_process(
            99,
            "other.exe",
            42,
            Some("app.exe"),
        ));
    }

    #[test]
    fn built_in_exclusions_include_system_processes() {
        assert!(is_builtin_excluded("csrss.exe"));
        assert!(is_builtin_excluded("winlogon.exe"));
        assert!(!is_builtin_excluded("browser.exe"));
    }
}
