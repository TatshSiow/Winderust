use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::c_void,
    ptr::null,
    time::{Duration, Instant},
};

use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, ERROR_ACCESS_DENIED, HANDLE},
    System::{
        JobObjects::{AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject},
        RemoteDesktop::ProcessIdToSessionId,
        Threading::{GetCurrentProcessId, OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE},
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
    pub temporary_thawed_processes: usize,
    pub tracked_apps: Vec<String>,
    pub suspended_apps: Vec<String>,
    pub temporary_thawed_apps: Vec<String>,
    pub skipped_processes: usize,
    pub failed_actions: usize,
    pub message: String,
    pub last_error: Option<String>,
}

#[derive(Default)]
pub struct AppSuspensionManager {
    tracked: BTreeMap<u32, TrackedProcess>,
    suspended: BTreeMap<u32, SuspendedProcess>,
    temporary_thawed: BTreeMap<u32, TemporaryThaw>,
}

struct TrackedProcess {
    process_name: String,
    background_since: Instant,
}

struct SuspendedProcess {
    process_name: String,
    freezer: ProcessFreezer,
    suspended_since: Instant,
}

struct TemporaryThaw {
    process_name: String,
    thaw_until: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TemporaryThawState {
    None,
    Active,
    Expired,
}

impl AppSuspensionManager {
    pub fn update(
        &mut self,
        settings: &AppSuspensionSettings,
        automation_enabled: bool,
        foreground_process_id: Option<u32>,
    ) -> AppSuspensionSnapshot {
        let now = Instant::now();

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

        let mut failed_actions = 0;

        let Some(foreground_process_id) = foreground_process_id else {
            failed_actions += self.clear_all();
            return AppSuspensionSnapshot {
                enabled: true,
                failed_actions,
                message: "Paused: foreground app is unknown.".to_owned(),
                ..Default::default()
            };
        };

        let current_process_id = unsafe { GetCurrentProcessId() };
        let Some(current_session_id) = process_session_id(current_process_id) else {
            failed_actions += self.clear_all();
            return AppSuspensionSnapshot {
                enabled: true,
                failed_actions,
                message: "Paused: current Windows session is unknown.".to_owned(),
                ..Default::default()
            };
        };

        let processes = match list_processes() {
            Ok(processes) => processes,
            Err(err) => {
                failed_actions += self.clear_all();
                return AppSuspensionSnapshot {
                    enabled: true,
                    failed_actions,
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
        let mut target_processes = BTreeMap::new();

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

            target_processes.insert(process.id, process.name);
        }

        let target_ids = target_processes.keys().copied().collect::<BTreeSet<_>>();
        failed_actions += self.release_non_targets(&target_ids);
        self.tracked
            .retain(|process_id, _process| target_ids.contains(process_id));
        self.temporary_thawed
            .retain(|process_id, _process| target_ids.contains(process_id));
        failed_actions += self.release_for_temporary_thaw(settings, &target_ids, now);

        let mut skipped_processes = 0;
        let mut last_error = None;

        for (process_id, process_name) in target_processes {
            if self.suspended.contains_key(&process_id) {
                continue;
            }

            match self.temporary_thaw_state(process_id, &process_name, now) {
                TemporaryThawState::Active => continue,
                TemporaryThawState::Expired => {
                    self.tracked.remove(&process_id);
                }
                TemporaryThawState::None => {
                    let tracked =
                        self.tracked
                            .entry(process_id)
                            .or_insert_with(|| TrackedProcess {
                                process_name: process_name.clone(),
                                background_since: now,
                            });
                    tracked.process_name = process_name.clone();
                    if tracked.background_since.elapsed() < delay {
                        continue;
                    }
                }
            }

            match suspend_process(process_id, process_name, now) {
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
            temporary_thawed_processes: self.temporary_thawed.len(),
            tracked_apps: unique_app_names(
                self.tracked
                    .values()
                    .map(|process| process.process_name.as_str()),
            ),
            suspended_apps: unique_app_names(
                self.suspended
                    .values()
                    .map(|process| process.process_name.as_str()),
            ),
            temporary_thawed_apps: unique_app_names(
                self.temporary_thawed
                    .values()
                    .map(|process| process.process_name.as_str()),
            ),
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
        self.temporary_thawed.clear();
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

    fn release_for_temporary_thaw(
        &mut self,
        settings: &AppSuspensionSettings,
        target_ids: &BTreeSet<u32>,
        now: Instant,
    ) -> usize {
        if !settings.temporary_thaw_enabled
            || settings.temporary_thaw_interval_seconds == 0
            || settings.temporary_thaw_duration_seconds == 0
        {
            return 0;
        }

        let interval = Duration::from_secs(settings.temporary_thaw_interval_seconds);
        let duration = Duration::from_secs(settings.temporary_thaw_duration_seconds);
        let process_ids = self
            .suspended
            .iter()
            .filter(|(process_id, process)| {
                target_ids.contains(process_id)
                    && now.duration_since(process.suspended_since) >= interval
            })
            .map(|(process_id, _process)| *process_id)
            .collect::<Vec<_>>();

        let mut failed = 0;
        for process_id in process_ids {
            if let Some(process) = self.suspended.remove(&process_id) {
                let process_name = process.process_name.clone();
                if resume_process(process).is_err() {
                    failed += 1;
                } else {
                    self.temporary_thawed.insert(
                        process_id,
                        TemporaryThaw {
                            process_name,
                            thaw_until: now + duration,
                        },
                    );
                }
            }
        }

        failed
    }

    fn temporary_thaw_state(
        &mut self,
        process_id: u32,
        process_name: &str,
        now: Instant,
    ) -> TemporaryThawState {
        let Some(thaw) = self.temporary_thawed.get_mut(&process_id) else {
            return TemporaryThawState::None;
        };

        if now < thaw.thaw_until {
            thaw.process_name = process_name.to_owned();
            TemporaryThawState::Active
        } else {
            self.temporary_thawed.remove(&process_id);
            TemporaryThawState::Expired
        }
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
            temporary_thawed_processes: 0,
            tracked_apps: Vec::new(),
            suspended_apps: Vec::new(),
            temporary_thawed_apps: Vec::new(),
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

fn unique_app_names<'a>(names: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut unique = BTreeMap::new();
    for name in names {
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            unique
                .entry(trimmed.to_ascii_lowercase())
                .or_insert_with(|| trimmed.to_owned());
        }
    }

    unique.into_values().collect()
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

const JOB_OBJECT_FREEZE_INFORMATION_CLASS: i32 = 18;

#[repr(C)]
struct JobObjectFreezeInformation {
    flags: u32,
    freeze: u8,
    swap: u8,
    spare: u16,
    wake_filter_high: u32,
    wake_filter_low: u32,
}

struct ProcessFreezer {
    job_handle: HANDLE,
}

impl ProcessFreezer {
    fn freeze(process_id: u32) -> Result<Self, SuspensionError> {
        let process_handle =
            unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, process_id) };
        if process_handle.is_null() {
            return Err(open_process_error(process_id));
        }

        let job_handle = unsafe { CreateJobObjectW(null(), null()) };
        if job_handle.is_null() {
            let error = last_error();
            unsafe {
                CloseHandle(process_handle);
            }
            return Err(SuspensionError::Failed(format!(
                "CreateJobObjectW failed with error {error}."
            )));
        }

        let assigned = unsafe { AssignProcessToJobObject(job_handle, process_handle) != 0 };
        unsafe {
            CloseHandle(process_handle);
        }
        if !assigned {
            let error = last_error();
            unsafe {
                CloseHandle(job_handle);
            }
            return Err(SuspensionError::Failed(format!(
                "AssignProcessToJobObject({process_id}) failed with error {error}."
            )));
        }

        let freezer = Self { job_handle };
        if let Err(err) = freezer.set_frozen(true) {
            drop(freezer);
            return Err(err);
        }

        Ok(freezer)
    }

    fn thaw(mut self) -> Result<(), SuspensionError> {
        let result = self.set_frozen(false);
        self.close();
        result
    }

    fn set_frozen(&self, frozen: bool) -> Result<(), SuspensionError> {
        let mut info = JobObjectFreezeInformation {
            flags: 1,
            freeze: u8::from(frozen),
            swap: 0,
            spare: 0,
            wake_filter_high: 0,
            wake_filter_low: 0,
        };

        let ok = unsafe {
            SetInformationJobObject(
                self.job_handle,
                JOB_OBJECT_FREEZE_INFORMATION_CLASS,
                &mut info as *mut _ as *mut c_void,
                std::mem::size_of::<JobObjectFreezeInformation>() as u32,
            )
        };

        if ok == 0 {
            Err(SuspensionError::Failed(format!(
                "SetInformationJobObject freeze={} failed with error {}.",
                frozen,
                last_error()
            )))
        } else {
            Ok(())
        }
    }

    fn close(&mut self) {
        if !self.job_handle.is_null() {
            unsafe {
                CloseHandle(self.job_handle);
            }
            self.job_handle = null_mut_handle();
        }
    }
}

impl Drop for ProcessFreezer {
    fn drop(&mut self) {
        if !self.job_handle.is_null() {
            let _ = self.set_frozen(false);
            self.close();
        }
    }
}

fn null_mut_handle() -> HANDLE {
    std::ptr::null_mut()
}

fn open_process_error(process_id: u32) -> SuspensionError {
    let error = last_error();
    if error == ERROR_ACCESS_DENIED {
        SuspensionError::AccessDenied
    } else {
        SuspensionError::Failed(format!(
            "OpenProcess({process_id}) failed with error {error}."
        ))
    }
}

fn suspend_process(
    process_id: u32,
    process_name: String,
    suspended_since: Instant,
) -> Result<SuspendedProcess, SuspensionError> {
    let freezer = ProcessFreezer::freeze(process_id)?;
    Ok(SuspendedProcess {
        process_name,
        freezer,
        suspended_since,
    })
}

fn resume_process(process: SuspendedProcess) -> Result<(), SuspensionError> {
    process.freezer.thaw()
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
    fn temporary_thaw_state_blocks_until_window_expires() {
        let mut manager = AppSuspensionManager::default();
        let now = Instant::now();
        manager.temporary_thawed.insert(
            7,
            TemporaryThaw {
                process_name: "chat.exe".to_owned(),
                thaw_until: now + Duration::from_secs(5),
            },
        );

        assert_eq!(
            manager.temporary_thaw_state(7, "CHAT.EXE", now),
            TemporaryThawState::Active
        );
        assert_eq!(
            manager.temporary_thawed.get(&7).unwrap().process_name,
            "CHAT.EXE"
        );
        assert_eq!(
            manager.temporary_thaw_state(7, "chat.exe", now + Duration::from_secs(6)),
            TemporaryThawState::Expired
        );
        assert!(!manager.temporary_thawed.contains_key(&7));
    }

    #[test]
    fn temporary_thaw_state_reports_none_without_entry() {
        let mut manager = AppSuspensionManager::default();

        assert_eq!(
            manager.temporary_thaw_state(99, "chat.exe", Instant::now()),
            TemporaryThawState::None
        );
    }

    #[test]
    fn built_in_exclusions_include_system_processes() {
        assert!(is_builtin_excluded("csrss.exe"));
        assert!(is_builtin_excluded("winlogon.exe"));
        assert!(!is_builtin_excluded("browser.exe"));
        assert!(!is_builtin_excluded("ms-teams.exe"));
    }
}
