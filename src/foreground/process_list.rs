use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::Instant,
};
#[cfg(test)]
use windows_sys::Win32::System::Threading::GetCurrentProcessId;

use crate::{
    cpu::ProcessCpuSample,
    platform::windows::sid_bytes,
    win_util::{filetime_to_u64, WinHandle},
};

use windows_sys::Win32::{
    Foundation::{
        GetLastError, ERROR_INSUFFICIENT_BUFFER, ERROR_NO_MORE_FILES, FILETIME, HANDLE,
        INVALID_HANDLE_VALUE, NTSTATUS,
    },
    Security::{
        GetTokenInformation, IsWellKnownSid, LookupAccountSidW, TokenUser, WinLocalServiceSid,
        WinLocalSystemSid, WinNetworkServiceSid, PSID, SID_NAME_USE, TOKEN_QUERY, TOKEN_USER,
    },
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        },
        ProcessStatus::{K32EnumProcesses, K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX},
        RemoteDesktop::ProcessIdToSessionId,
        SystemInformation::GetSystemWindowsDirectoryW,
        Threading::{
            GetPriorityClass, GetProcessInformation, GetProcessTimes, IsProcessCritical,
            OpenProcess, OpenProcessToken, ProcessPowerThrottling, ProcessProtectionLevelInfo,
            QueryFullProcessImageNameW, IDLE_PRIORITY_CLASS, PROCESS_NAME_WIN32,
            PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            PROCESS_POWER_THROTTLING_STATE, PROCESS_PROTECTION_LEVEL_INFORMATION,
            PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION, PROCESS_SET_QUOTA,
            PROCESS_TERMINATE, PROTECTION_LEVEL_NONE,
        },
        WindowsProgramming::PUBLIC_OBJECT_BASIC_INFORMATION,
    },
};

unsafe extern "system" {
    fn NtQueryObject(
        handle: HANDLE,
        object_information_class: i32,
        object_information: *mut core::ffi::c_void,
        object_information_length: u32,
        return_length: *mut u32,
    ) -> NTSTATUS;
}

const OBJECT_BASIC_INFORMATION_CLASS: i32 = 0;
const PROCESS_IMAGE_PATH_INITIAL_BUFFER_LEN: usize = 512;
const PROCESS_IMAGE_PATH_MAX_BUFFER_LEN: usize = 32_768;
const WINDOWS_DIRECTORY_INITIAL_BUFFER_LEN: usize = 260;

pub const CORE_BUILT_IN_PROCESS_EXCLUSIONS: &[&str] = &[
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
    "services.exe",
    "sihost.exe",
    "smss.exe",
    "system",
    "taskmgr.exe",
    "wininit.exe",
    "winlogon.exe",
];

pub const EXTENDED_BUILT_IN_PROCESS_EXCLUSIONS: &[&str] = &[
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
pub struct ProcessInfo {
    pub id: u32,
    pub creation_time: Option<u64>,
    pub parent_id: Option<u32>,
    pub session_id: Option<u32>,
    pub user_name: Option<String>,
    pub is_service_account: Option<bool>,
    pub is_critical: Option<bool>,
    pub can_set_information: bool,
    pub name: String,
    pub image_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy)]
pub struct ProcessResourceSample {
    pub cpu: ProcessCpuSample,
    pub creation_time: u64,
    pub working_set_bytes: Option<u64>,
    pub private_commit_bytes: Option<u64>,
    pub efficiency_mode: Option<bool>,
}

#[derive(Clone, Copy)]
pub enum ProcessResourceSampling {
    CpuOnly,
    CpuAndMemory,
    WithEfficiencyMode,
}

impl ProcessResourceSampling {
    fn efficiency_mode(
        self,
        query_eco_qos: impl FnOnce() -> Option<bool>,
        query_priority_class: impl FnOnce() -> u32,
    ) -> Option<bool> {
        if !matches!(self, Self::WithEfficiencyMode) {
            return None;
        }
        if !query_eco_qos()? {
            return Some(false);
        }
        let priority_class = query_priority_class();
        (priority_class != 0).then_some(priority_class == IDLE_PRIORITY_CLASS)
    }
}

pub fn sample_process_resources(
    processes: &[ProcessInfo],
    sampling: ProcessResourceSampling,
) -> BTreeMap<u32, ProcessResourceSample> {
    processes
        .iter()
        .filter_map(|process| {
            let creation_time = process.creation_time?;
            let sample = sample_process_resource(process.id, sampling)?;
            (sample.creation_time == creation_time).then_some((process.id, sample))
        })
        .collect()
}

fn sample_process_resource(
    process_id: u32,
    sampling: ProcessResourceSampling,
) -> Option<ProcessResourceSample> {
    // SAFETY: process_id came from the current process snapshot and the handle is not inherited.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    let handle = (!handle.is_null()).then(|| WinHandle::new(handle))?;
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: handle is live and all FILETIME outputs are writable for the call.
    if unsafe {
        GetProcessTimes(
            handle.raw(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    } == 0
    {
        return None;
    }

    let memory_size = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
    let mut memory = PROCESS_MEMORY_COUNTERS_EX {
        cb: memory_size,
        ..Default::default()
    };
    let memory_available = if matches!(sampling, ProcessResourceSampling::CpuOnly) {
        false
    } else {
        // SAFETY: handle is live and the extended counter buffer is writable for this size.
        unsafe {
            K32GetProcessMemoryInfo(
                handle.raw(),
                std::ptr::from_mut(&mut memory).cast(),
                memory_size,
            ) != 0
        }
    };

    let efficiency_mode = sampling.efficiency_mode(
        || {
            let mut power_throttling = PROCESS_POWER_THROTTLING_STATE {
                Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
                ..Default::default()
            };
            // SAFETY: handle is live and power_throttling is writable for the supplied structure size.
            (unsafe {
                GetProcessInformation(
                    handle.raw(),
                    ProcessPowerThrottling,
                    (&mut power_throttling as *mut PROCESS_POWER_THROTTLING_STATE).cast(),
                    std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
                )
            } != 0)
                .then_some(
                    power_throttling.ControlMask & PROCESS_POWER_THROTTLING_EXECUTION_SPEED != 0
                        && power_throttling.StateMask & PROCESS_POWER_THROTTLING_EXECUTION_SPEED
                            != 0,
                )
        },
        || {
            // SAFETY: handle is live and GetPriorityClass only reads process state.
            unsafe { GetPriorityClass(handle.raw()) }
        },
    );

    Some(ProcessResourceSample {
        cpu: ProcessCpuSample {
            cpu_time_100ns: filetime_to_u64(kernel).saturating_add(filetime_to_u64(user)),
            sampled_at: Instant::now(),
        },
        creation_time: filetime_to_u64(creation),
        working_set_bytes: memory_available.then_some(memory.WorkingSetSize as u64),
        private_commit_bytes: memory_available.then_some(memory.PrivateUsage as u64),
        efficiency_mode,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessActionTarget {
    pub id: u32,
    pub name: String,
    pub executable_path: PathBuf,
    pub creation_time: u64,
    pub(crate) session_id: Option<u32>,
    pub(crate) is_service_account: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProcessActionAccess {
    SafetyOnly,
    SetInformation,
    TrimWorkingSet,
    Terminate,
    AssignToJob,
}

impl ProcessActionAccess {
    fn desired_access(self) -> u32 {
        match self {
            Self::SafetyOnly => 0,
            Self::SetInformation => PROCESS_SET_INFORMATION,
            Self::TrimWorkingSet => PROCESS_SET_QUOTA,
            Self::Terminate => PROCESS_TERMINATE,
            Self::AssignToJob => PROCESS_SET_QUOTA | PROCESS_TERMINATE,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProcessActionTargetError {
    #[error("Winderust cannot modify this process.")]
    ProtectedProcess,
    #[cfg(test)]
    #[error("Could not determine the current Windows session.")]
    CurrentSessionUnavailable,
    #[cfg(test)]
    #[error("Processes in another Windows session cannot be modified.")]
    DifferentSession,
    #[cfg(test)]
    #[error("The selected process instance has changed.")]
    ProcessChanged,
    #[cfg(test)]
    #[error("The selected process is no longer available (Win32 error {0}).")]
    ProcessUnavailable(u32),
    #[error("Could not identify the selected process instance.")]
    IdentityUnavailable,
}

#[cfg(test)]
pub fn capture_process_action_target(
    process_id: u32,
    expected_executable_path: &Path,
    allow_cross_session: bool,
) -> Result<ProcessActionTarget, ProcessActionTargetError> {
    capture_process_action_target_inner(
        process_id,
        expected_executable_path,
        allow_cross_session,
        true,
    )
}

#[cfg(test)]
fn capture_process_action_target_inner(
    process_id: u32,
    expected_executable_path: &Path,
    allow_cross_session: bool,
    enforce_acquisition_safety: bool,
) -> Result<ProcessActionTarget, ProcessActionTargetError> {
    // SAFETY: GetCurrentProcessId takes no arguments and has no caller requirements.
    let current_process_id = unsafe { GetCurrentProcessId() };
    if process_id == 0 || process_id == current_process_id {
        return Err(ProcessActionTargetError::ProtectedProcess);
    }
    let session_id = process_session_id(process_id);
    if !allow_cross_session {
        let current_session_id = process_session_id(current_process_id)
            .ok_or(ProcessActionTargetError::CurrentSessionUnavailable)?;
        if session_id != Some(current_session_id) {
            return Err(ProcessActionTargetError::DifferentSession);
        }
    }
    if !expected_executable_path.is_absolute() {
        return Err(ProcessActionTargetError::IdentityUnavailable);
    }
    // SAFETY: process_id came from the current process snapshot and no inherited handle is
    // requested.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if handle.is_null() {
        // SAFETY: GetLastError has no caller requirements and is read immediately after the
        // failing OpenProcess call on this thread.
        return Err(ProcessActionTargetError::ProcessUnavailable(unsafe {
            GetLastError()
        }));
    }
    let process = WinHandle::new(handle);
    if enforce_acquisition_safety && process_critical_from_handle(&process) != Some(false) {
        return Err(ProcessActionTargetError::ProtectedProcess);
    }
    let executable_path = process_image_path_from_handle(&process)
        .ok_or(ProcessActionTargetError::IdentityUnavailable)?;
    if !same_executable_path(&executable_path, expected_executable_path) {
        return Err(ProcessActionTargetError::ProcessChanged);
    }
    let name = executable_path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or(ProcessActionTargetError::IdentityUnavailable)?
        .to_ascii_lowercase();
    let creation_time = process
        .process_creation_time()
        .ok_or(ProcessActionTargetError::IdentityUnavailable)?;
    let is_service_account = process_runs_as_service_account_from_handle(&process);
    Ok(ProcessActionTarget {
        id: process_id,
        name,
        executable_path,
        creation_time,
        session_id,
        is_service_account,
    })
}

pub fn process_tree_action_targets(
    captured_roots: &[ProcessActionTarget],
    processes: &[ProcessInfo],
) -> Result<Vec<ProcessActionTarget>, String> {
    if captured_roots.is_empty() {
        return Err("No process targets were available.".to_owned());
    }

    for captured_root in captured_roots {
        let current_root = processes
            .iter()
            .find(|process| process.id == captured_root.id)
            .ok_or_else(|| {
                "A selected process exited before its tree could be stopped.".to_owned()
            })?;
        let current_root = process_action_target_from_snapshot(current_root)?;
        if current_root.creation_time != captured_root.creation_time
            || !same_process_name(&current_root.name, &captured_root.name)
            || !same_executable_path(
                &current_root.executable_path,
                &captured_root.executable_path,
            )
        {
            return Err(
                "A selected process instance changed before its tree could be stopped.".to_owned(),
            );
        }
    }

    let root_ids = captured_roots
        .iter()
        .map(|target| target.id)
        .collect::<Vec<_>>();
    let process_ids = process_tree_ids_postorder(&root_ids, processes)?;

    let mut targets = Vec::with_capacity(process_ids.len());
    for process_id in process_ids {
        let process = processes
            .iter()
            .find(|process| process.id == process_id)
            .ok_or_else(|| "A process exited before its tree could be stopped.".to_owned())?;
        targets.push(process_action_target_from_snapshot(process)?);
    }
    Ok(targets)
}

fn process_action_target_from_snapshot(
    process: &ProcessInfo,
) -> Result<ProcessActionTarget, String> {
    let executable_path = process
        .image_path
        .clone()
        .ok_or_else(|| "A process could not be identified safely.".to_owned())?;
    if !executable_path.is_absolute() {
        return Err("A process could not be identified safely.".to_owned());
    }
    let creation_time = process
        .creation_time
        .ok_or_else(|| "A process instance could not be identified safely.".to_owned())?;
    let name = executable_path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "A process could not be identified safely.".to_owned())?
        .to_ascii_lowercase();
    if !same_process_name(&name, &process.name) {
        return Err("A process instance changed before it could be stopped.".to_owned());
    }
    Ok(ProcessActionTarget {
        id: process.id,
        name,
        executable_path,
        creation_time,
        session_id: process.session_id,
        is_service_account: process.is_service_account,
    })
}

fn process_tree_ids_postorder(
    root_ids: &[u32],
    processes: &[ProcessInfo],
) -> Result<Vec<u32>, String> {
    let mut selected = BTreeSet::new();
    let mut ordered = Vec::new();
    let mut pending = root_ids
        .iter()
        .rev()
        .map(|process_id| (*process_id, false))
        .collect::<Vec<_>>();
    while let Some((process_id, expanded)) = pending.pop() {
        if expanded {
            ordered.push(process_id);
            continue;
        }
        if !selected.insert(process_id) {
            continue;
        }
        let parent_creation_time = processes
            .iter()
            .find(|process| process.id == process_id)
            .ok_or_else(|| "A process exited before its tree could be stopped.".to_owned())?
            .creation_time
            .ok_or_else(|| {
                "A process tree relationship could not be verified safely.".to_owned()
            })?;
        pending.push((process_id, true));
        for child in processes
            .iter()
            .filter(|process| process.parent_id == Some(process_id))
        {
            let child_creation_time = child.creation_time.ok_or_else(|| {
                "A process tree relationship could not be verified safely.".to_owned()
            })?;
            if child_creation_time >= parent_creation_time {
                pending.push((child.id, false));
            }
        }
    }
    Ok(ordered)
}

#[cfg(test)]
pub(crate) fn ensure_process_action_target_access(
    target: &ProcessActionTarget,
    access: ProcessActionAccess,
) -> Result<(), String> {
    let process = open_process_for_query(target.id)
        .ok_or_else(|| "The process is no longer accessible.".to_owned())?;
    ensure_process_action_target_safety_on_handle(target, &process)?;
    let desired_access = access.desired_access();
    if desired_access != 0 && !process_has_access(target.id, desired_access) {
        return Err("Windows denied process control access.".to_owned());
    }
    Ok(())
}

pub(crate) fn ensure_process_action_target_access_on_handle(
    target: &ProcessActionTarget,
    access: ProcessActionAccess,
    process: &WinHandle,
) -> Result<(), String> {
    ensure_process_action_target_safety_on_handle(target, process)?;
    let desired_access = access.desired_access();
    if desired_access != 0 && process_handle_has_access(process, desired_access) != Some(true) {
        return Err("Windows denied process control access.".to_owned());
    }
    Ok(())
}

fn ensure_process_action_target_safety_on_handle(
    target: &ProcessActionTarget,
    process: &WinHandle,
) -> Result<(), String> {
    if contains_process_name(CORE_BUILT_IN_PROCESS_EXCLUSIONS, &target.name) {
        return Err("Built-in Windows processes cannot be modified.".to_owned());
    }
    if process_critical_from_handle(process) != Some(false) {
        return Err("Critical or unverifiable processes cannot be modified.".to_owned());
    }
    if process_protection_from_handle(process) != Some(false) {
        return Err("Windows protected processes cannot be modified.".to_owned());
    }
    Ok(())
}

pub fn open_process_properties(executable_path: &Path) -> Result<(), String> {
    use windows_sys::Win32::{
        System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED},
        UI::Shell::{SHObjectProperties, SHOP_FILEPATH},
    };
    if !executable_path.is_absolute() || !executable_path.is_file() {
        return Err("The process executable path is unavailable.".to_owned());
    }
    let path = executable_path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // SAFETY: No reserved pointer is supplied; this background thread owns the COM apartment.
    let initialized = unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) };
    if initialized < 0 {
        return Err(format!(
            "Could not initialize Windows properties: {initialized:#x}."
        ));
    }
    // SAFETY: path is terminated UTF-16, no parent is required, and null selects the default page.
    let opened = unsafe {
        SHObjectProperties(
            std::ptr::null_mut(),
            SHOP_FILEPATH as u32,
            path.as_ptr(),
            std::ptr::null(),
        )
    };
    // SAFETY: Balance the successful COM initialization on this same thread.
    unsafe {
        CoUninitialize();
    }
    if opened == 0 {
        Err("Could not open the executable properties.".to_owned())
    } else {
        Ok(())
    }
}

pub fn open_process_location(executable_path: &Path) -> Result<(), String> {
    if !executable_path.is_absolute() {
        return Err("The process executable path is unavailable.".to_owned());
    }
    let mut argument = OsString::from("/select,");
    argument.push(executable_path);
    Command::new(windows_explorer_path()?)
        .arg(argument)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Could not open the process location: {error}."))
}

fn windows_explorer_path() -> Result<PathBuf, String> {
    let mut buffer = vec![0u16; WINDOWS_DIRECTORY_INITIAL_BUFFER_LEN];
    loop {
        // SAFETY: buffer is writable for its declared length and GetSystemWindowsDirectoryW
        // writes at most that many UTF-16 code units.
        let length =
            unsafe { GetSystemWindowsDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
        if length == 0 {
            // SAFETY: GetLastError reads thread-local state immediately after the failed call.
            let error = unsafe { GetLastError() };
            return Err(format!(
                "Could not locate Windows Explorer (Win32 error {error})."
            ));
        }
        let length = length as usize;
        if length < buffer.len() {
            let mut path = PathBuf::from(OsString::from_wide(&buffer[..length]));
            path.push("explorer.exe");
            return Ok(path);
        }
        buffer.resize(length.saturating_add(1), 0);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessCandidateInfo {
    pub name: String,
    pub image_path: PathBuf,
    pub has_suspendable_instance: bool,
}

pub fn list_process_candidates() -> Result<Vec<ProcessCandidateInfo>, String> {
    let mut processes = list_processes()?;
    enrich_process_paths(&mut processes);
    Ok(process_candidates_from_processes(&processes))
}

pub fn process_candidates_from_processes(processes: &[ProcessInfo]) -> Vec<ProcessCandidateInfo> {
    let mut candidates: BTreeMap<OsString, ProcessCandidateInfo> = BTreeMap::new();
    for process in processes {
        if process.is_critical != Some(false) || !process.can_set_information {
            continue;
        }
        let Some(image_path) = process.image_path.as_ref() else {
            continue;
        };
        let has_suspendable_instance = process.session_id.is_some_and(|session_id| session_id != 0)
            && process.is_service_account == Some(false);
        candidates
            .entry(executable_grouping_key(image_path))
            .and_modify(|candidate| {
                candidate.has_suspendable_instance |= has_suspendable_instance;
            })
            .or_insert_with(|| ProcessCandidateInfo {
                name: process.name.clone(),
                image_path: image_path.clone(),
                has_suspendable_instance,
            });
    }
    candidates.into_values().collect()
}

pub fn list_processes() -> Result<Vec<ProcessInfo>, String> {
    let mut processes = list_process_entries()?;
    for process in &mut processes {
        process.session_id = process_session_id(process.id);
        let Some(handle) = open_process_for_query(process.id) else {
            continue;
        };
        process.creation_time = handle.process_creation_time();
        process.is_critical = process_critical_from_handle(&handle);
        process.can_set_information = process_protection_from_handle(&handle) == Some(false)
            && process_has_access(process.id, PROCESS_SET_INFORMATION);
    }
    Ok(processes)
}

pub(crate) fn list_process_ancestry(root: u32) -> Result<Vec<ProcessInfo>, String> {
    let mut processes = list_process_entries()?;
    let mut children = BTreeMap::<u32, Vec<u32>>::new();
    for process in &processes {
        if let Some(parent) = process.parent_id {
            children.entry(parent).or_default().push(process.id);
        }
    }
    let mut pending = vec![root];
    let mut candidates = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if candidates.insert(id) {
            pending.extend(children.remove(&id).into_iter().flatten());
        }
    }
    processes.retain(|process| candidates.contains(&process.id));
    for process in &mut processes {
        process.creation_time =
            open_process_for_query(process.id).and_then(|handle| handle.process_creation_time());
    }
    Ok(processes)
}

fn list_process_entries() -> Result<Vec<ProcessInfo>, String> {
    let snapshot = process_snapshot()?;
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut processes = Vec::new();

    // SAFETY: snapshot is live and entry declares its size and remains writable.
    let mut has_entry = unsafe { Process32FirstW(snapshot.raw(), &mut entry) != 0 };
    while has_entry {
        if let Some(name) = process_name_from_entry(&entry) {
            processes.push(ProcessInfo {
                id: entry.th32ProcessID,
                creation_time: None,
                parent_id: (entry.th32ParentProcessID != 0).then_some(entry.th32ParentProcessID),
                session_id: None,
                user_name: None,
                is_service_account: None,
                is_critical: None,
                can_set_information: false,
                name,
                image_path: None,
            });
        }

        // SAFETY: snapshot remains live and entry remains writable for the next record.
        has_entry = unsafe { Process32NextW(snapshot.raw(), &mut entry) != 0 };
    }
    ensure_process_iteration_complete()?;

    Ok(processes)
}

pub fn list_processes_with_paths() -> Result<Vec<ProcessInfo>, String> {
    let mut processes = list_processes()?;
    let mut account_names = BTreeMap::new();
    enrich_process_metadata(&mut processes, |handle| {
        process_identity_from_handle(handle, &mut account_names)
    });
    Ok(processes)
}

pub(crate) fn enrich_process_paths(processes: &mut [ProcessInfo]) {
    // Policy and picker scans need SID safety classification, not display names.
    enrich_process_metadata(processes, |handle| {
        (None, process_runs_as_service_account_from_handle(handle))
    });
}

fn enrich_process_metadata(
    processes: &mut [ProcessInfo],
    mut identity: impl FnMut(&WinHandle) -> (Option<String>, Option<bool>),
) {
    for process in processes {
        let Some(handle) = open_process_for_query(process.id) else {
            continue;
        };
        process.image_path = process_image_path_from_handle(&handle);
        process.creation_time = handle.process_creation_time();
        (process.user_name, process.is_service_account) = identity(&handle);
        process.is_critical = process_critical_from_handle(&handle);
    }
}

pub fn for_each_process_id(visit: impl FnMut(u32)) -> Result<(), String> {
    let ids = read_process_ids(|ids, bytes| {
        let mut returned = 0;
        // SAFETY: ids is writable for bytes and returned receives the initialized byte count.
        if unsafe { K32EnumProcesses(ids.as_mut_ptr(), bytes, &mut returned) } == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(returned)
    })?;
    ids.into_iter().for_each(visit);
    Ok(())
}

fn read_process_ids(
    mut read: impl FnMut(&mut [u32], u32) -> Result<u32, String>,
) -> Result<Vec<u32>, String> {
    const PID_BYTES: u32 = std::mem::size_of::<u32>() as u32;
    let mut bytes = 512 * PID_BYTES;
    let mut ids = Vec::new();
    loop {
        let slots = (bytes / PID_BYTES) as usize;
        ids.try_reserve_exact(slots - ids.len())
            .map_err(|error| error.to_string())?;
        ids.resize(slots, 0);
        let returned = read(&mut ids, bytes)?;
        if returned > bytes || !returned.is_multiple_of(PID_BYTES) {
            return Err("Windows returned an invalid process ID count.".to_owned());
        }
        if returned < bytes {
            ids.truncate((returned / PID_BYTES) as usize);
            return Ok(ids);
        }
        // Windows reports a full buffer even when it did not fit the complete list.
        bytes = bytes
            .checked_mul(2)
            .ok_or("The process ID list is too large.")?;
    }
}

fn ensure_process_iteration_complete() -> Result<(), String> {
    // SAFETY: GetLastError takes no arguments and reads thread-local state immediately after
    // process enumeration.
    let error = unsafe { GetLastError() };
    if error == ERROR_NO_MORE_FILES {
        Ok(())
    } else {
        Err(std::io::Error::from_raw_os_error(error as i32).to_string())
    }
}

pub fn process_session_id(process_id: u32) -> Option<u32> {
    let mut session_id = 0;
    // SAFETY: session_id is writable and process_id is a value, not a borrowed handle.
    let ok = unsafe { ProcessIdToSessionId(process_id, &mut session_id) };
    (ok != 0).then_some(session_id)
}

pub fn is_foreground_process(
    process_id: u32,
    executable_path: &Path,
    foreground_process_id: Option<u32>,
    foreground_executable_path: Option<&Path>,
) -> bool {
    Some(process_id) == foreground_process_id
        || foreground_executable_path
            .is_some_and(|foreground| same_executable_path(foreground, executable_path))
}

pub fn should_ignore_foreground_process(
    exclude_foreground_app: bool,
    process_id: u32,
    executable_path: &Path,
    foreground_process_id: Option<u32>,
    foreground_executable_path: Option<&Path>,
) -> bool {
    exclude_foreground_app
        && is_foreground_process(
            process_id,
            executable_path,
            foreground_process_id,
            foreground_executable_path,
        )
}

#[derive(Debug, Default)]
pub struct ProtectedProcesses {
    process_ids: Arc<BTreeSet<u32>>,
    foreground_process_id: Option<u32>,
    executable_paths: Vec<PathBuf>,
}

impl ProtectedProcesses {
    pub fn capture(
        processes: &[ProcessInfo],
        protect_foreground_app: bool,
        foreground_process_id: Option<u32>,
        visible_window_process_ids: impl Into<Arc<BTreeSet<u32>>>,
    ) -> Self {
        let visible_window_process_ids = visible_window_process_ids.into();
        let foreground_process_id = foreground_process_id.filter(|_| protect_foreground_app);
        let executable_paths = processes
            .iter()
            .filter(|process| {
                visible_window_process_ids.contains(&process.id)
                    || foreground_process_id == Some(process.id)
            })
            .filter_map(verified_process_executable_path)
            .map(Cow::into_owned)
            .collect();
        Self {
            process_ids: visible_window_process_ids,
            foreground_process_id,
            executable_paths,
        }
    }

    pub fn contains(&self, process_id: u32, executable_path: &Path) -> bool {
        self.process_ids.contains(&process_id)
            || self.foreground_process_id == Some(process_id)
            || self
                .executable_paths
                .iter()
                .any(|protected_path| same_executable_path(protected_path, executable_path))
    }

    /// A name match only permits a path lookup; it never establishes membership.
    pub(crate) fn may_contain(&self, process: &ProcessInfo) -> bool {
        self.process_ids.contains(&process.id)
            || self.foreground_process_id == Some(process.id)
            || process.image_path.is_some()
            || self.executable_paths.iter().any(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_none_or(|name| same_process_name(&process.name, name))
            })
    }
}

pub fn same_process_name(left: &str, right: &str) -> bool {
    left.trim().eq_ignore_ascii_case(right.trim())
}

pub fn executable_path_key(path: &Path) -> String {
    let path = path.to_string_lossy();
    normalize_path_slashes(path.trim()).into_owned()
}

pub(crate) fn executable_grouping_key(path: &Path) -> OsString {
    match path.to_str() {
        Some(path) => normalize_path_slashes(path).into_owned().into(),
        None => path.as_os_str().to_owned(),
    }
}

pub(crate) fn normalize_path_slashes(path: &str) -> Cow<'_, str> {
    if path.contains('/') {
        Cow::Owned(path.replace('/', std::path::MAIN_SEPARATOR_STR))
    } else {
        Cow::Borrowed(path)
    }
}

pub(crate) fn power_profile_executable_path_key(path: &str) -> Cow<'_, str> {
    let normalized = normalize_path_slashes(path);
    // Preserve verbatim paths, including spellings that normalize to the verbatim prefix.
    if normalized.starts_with(r"\\?\") {
        Cow::Borrowed(path)
    } else {
        normalized
    }
}

// Native executable paths preserve literal filename whitespace.
pub fn same_executable_path(left: &Path, right: &Path) -> bool {
    if left.as_os_str() == right.as_os_str() {
        return true;
    }
    // A lossy display string cannot establish identity between different native paths.
    let Some((left_text, right_text)) = left.to_str().zip(right.to_str()) else {
        return false;
    };
    let left_key = normalize_path_slashes(left_text);
    let right_key = normalize_path_slashes(right_text);
    if left_key == right_key {
        return true;
    }
    let ascii_case_match = left_key.eq_ignore_ascii_case(&right_key);
    if !ascii_case_match && !unicode_case_match(&left_key, &right_key) {
        return false;
    }

    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => {
            left.as_os_str() == right.as_os_str()
                || left
                    .to_str()
                    .zip(right.to_str())
                    .is_some_and(|(left, right)| {
                        normalize_path_slashes(left) == normalize_path_slashes(right)
                    })
        }
        (Err(_), Err(_)) => ascii_case_match,
        _ => false,
    }
}

// Ordinal equality makes Unicode spellings candidates for full path verification.
pub(crate) fn unicode_case_match(left: &str, right: &str) -> bool {
    if left.is_ascii() && right.is_ascii() {
        return false;
    }
    let mut left_buffer = [0; 128];
    let mut right_buffer = [0; 128];
    let left = utf16_for_ordinal(left, &mut left_buffer);
    let right = utf16_for_ordinal(right, &mut right_buffer);
    crate::platform::windows::ordinal_equal(&left, &right).unwrap_or(false)
}

fn utf16_for_ordinal<'a>(text: &str, buffer: &'a mut [u16]) -> Cow<'a, [u16]> {
    // The UTF-8 byte length bounds the UTF-16 unit count.
    if text.len() > buffer.len() {
        return Cow::Owned(text.encode_utf16().collect());
    }
    let mut len = 0;
    for unit in text.encode_utf16() {
        buffer[len] = unit;
        len += 1;
    }
    Cow::Borrowed(&buffer[..len])
}

pub(crate) fn same_executable_name(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right) || unicode_case_match(left, right)
}

pub(crate) fn path_has_executable_name(path: &Path, executable_name: &str) -> bool {
    path.is_absolute()
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| same_executable_name(name, executable_name))
}

pub(crate) fn executable_basename_may_match(process_name: &str, basename: &str) -> bool {
    same_executable_name(process_name.trim(), basename.trim())
}

pub fn process_matches_executable_path(process: &ProcessInfo, executable_path: &str) -> bool {
    let executable_path = Path::new(executable_path);
    let Some(file_name) = executable_path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if !executable_basename_may_match(&process.name, file_name) {
        return false;
    }

    verified_process_executable_path(process)
        .is_some_and(|actual_path| same_executable_path(&actual_path, executable_path))
}

pub fn process_executable_path(process: &ProcessInfo) -> Option<PathBuf> {
    verified_process_executable_path(process).map(Cow::into_owned)
}

pub fn contains_process_name<T: AsRef<str>>(list: &[T], process_name: &str) -> bool {
    list.iter()
        .any(|name| same_process_name(name.as_ref(), process_name))
}

pub fn process_failure_key(process_identity: &str) -> String {
    normalize_path_slashes(process_identity).into_owned()
}

pub fn process_count_label(count: usize) -> String {
    if count == 1 {
        "1 process".to_owned()
    } else {
        format!("{count} processes")
    }
}

fn process_name_from_entry(entry: &PROCESSENTRY32W) -> Option<String> {
    let len = entry
        .szExeFile
        .iter()
        .position(|code| *code == 0)
        .unwrap_or(entry.szExeFile.len());
    if len == 0 {
        return None;
    }

    let name = String::from_utf16_lossy(&entry.szExeFile[..len]).to_ascii_lowercase();
    (!is_system_process_name(&name)).then_some(name)
}

fn process_snapshot() -> Result<WinHandle, String> {
    // SAFETY: TH32CS_SNAPPROCESS ignores the process id argument and returns an owned handle.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        Err("Failed to read running process list.".to_owned())
    } else {
        Ok(WinHandle::new(snapshot))
    }
}

fn is_system_process_name(name: &str) -> bool {
    name.trim().eq_ignore_ascii_case("[system process]")
}

/// Resolves an absolute image path for one generation in the process catalog.
pub(crate) fn catalog_process_executable_path(
    processes: &[ProcessInfo],
    process_id: u32,
) -> Option<Cow<'_, Path>> {
    let process = processes.iter().find(|process| process.id == process_id)?;
    verified_process_executable_path(process)
}

pub(crate) fn verified_process_executable_path(process: &ProcessInfo) -> Option<Cow<'_, Path>> {
    process.creation_time?;
    process
        .image_path
        .as_deref()
        .map(Cow::Borrowed)
        .or_else(|| observed_process_executable_path(process).map(Cow::Owned))
        .filter(|path| path.is_absolute())
}

/// Reads the image only if the current handle still belongs to the observed process.
pub(crate) fn observed_process_executable_path(process: &ProcessInfo) -> Option<PathBuf> {
    let expected_creation = process.creation_time?;
    let handle = open_process_for_query(process.id)?;
    if handle.process_creation_time()? != expected_creation {
        return None;
    }
    process_image_path_from_handle(&handle)
}

pub(super) fn process_image_and_critical(process_id: u32) -> Option<(PathBuf, Option<bool>)> {
    let process = open_process_for_query(process_id)?;
    let path = process_image_path_from_handle(&process)?;
    Some((path, process_critical_from_handle(&process)))
}

fn open_process_for_query(process_id: u32) -> Option<WinHandle> {
    if process_id == 0 {
        return None;
    }

    // SAFETY: process_id came from a current snapshot and no inherited handle is requested.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    (!process.is_null()).then(|| WinHandle::new(process))
}

pub(crate) fn process_cpu_times(process_id: u32) -> Option<(u64, u64)> {
    open_process_for_query(process_id)?.process_times().ok()
}

fn process_has_access(process_id: u32, desired_access: u32) -> bool {
    // SAFETY: process_id came from a current snapshot and no inherited handle is requested.
    let process = unsafe { OpenProcess(desired_access, 0, process_id) };
    let process = (!process.is_null()).then(|| WinHandle::new(process));
    process
        .as_ref()
        .is_some_and(|process| process_handle_has_access(process, desired_access) == Some(true))
}

fn process_handle_has_access(process: &WinHandle, desired_access: u32) -> Option<bool> {
    let mut information = PUBLIC_OBJECT_BASIC_INFORMATION::default();
    // SAFETY: process is live and information is writable for its full declared size.
    let status = unsafe {
        NtQueryObject(
            process.raw(),
            OBJECT_BASIC_INFORMATION_CLASS,
            (&mut information as *mut PUBLIC_OBJECT_BASIC_INFORMATION).cast(),
            std::mem::size_of::<PUBLIC_OBJECT_BASIC_INFORMATION>() as u32,
            std::ptr::null_mut(),
        )
    };
    (status >= 0).then_some(information.GrantedAccess & desired_access == desired_access)
}

fn process_protection_from_handle(process: &WinHandle) -> Option<bool> {
    let mut protection = PROCESS_PROTECTION_LEVEL_INFORMATION::default();
    // SAFETY: process is live with query access and protection is writable for its full size.
    (unsafe {
        GetProcessInformation(
            process.raw(),
            ProcessProtectionLevelInfo,
            (&mut protection as *mut PROCESS_PROTECTION_LEVEL_INFORMATION).cast(),
            std::mem::size_of::<PROCESS_PROTECTION_LEVEL_INFORMATION>() as u32,
        )
    } != 0)
        .then_some(protection.ProtectionLevel != PROTECTION_LEVEL_NONE)
}

fn process_critical_from_handle(process: &WinHandle) -> Option<bool> {
    let mut critical = 0;
    // SAFETY: process is live with query access and critical is writable for the BOOL result.
    (unsafe { IsProcessCritical(process.raw(), &mut critical) } != 0).then_some(critical != 0)
}

fn process_identity_from_handle(
    process: &WinHandle,
    account_names: &mut BTreeMap<Vec<u8>, String>,
) -> (Option<String>, Option<bool>) {
    let Some(token_buffer) = process_token_user_buffer(process) else {
        return (None, None);
    };
    // SAFETY: successful TokenUser output starts with a valid aligned TOKEN_USER whose SID remains
    // valid while token_buffer is alive.
    let token_user = unsafe { &*token_buffer.as_ptr().cast::<TOKEN_USER>() };
    let Ok(sid) = sid_bytes(token_user.User.Sid, &token_buffer) else {
        return (None, None);
    };
    let native_sid = sid.as_ptr().cast_mut().cast();
    let name = cached_account_name(account_names, sid, || {
        process_user_name_from_sid(native_sid)
    });
    (name, Some(sid_is_service_account(native_sid)))
}

fn cached_account_name(
    account_names: &mut BTreeMap<Vec<u8>, String>,
    sid: &[u8],
    lookup: impl FnOnce() -> Option<String>,
) -> Option<String> {
    if let Some(name) = account_names.get(sid) {
        return Some(name.clone());
    }
    let name = lookup()?;
    account_names.insert(sid.to_vec(), name.clone());
    Some(name)
}

fn process_user_name_from_sid(sid: PSID) -> Option<String> {
    let mut name_len = 0;
    let mut domain_len = 0;
    let mut sid_use: SID_NAME_USE = 0;
    // SAFETY: the SID comes from the live TokenUser buffer; null outputs request required lengths.
    let first_result = unsafe {
        LookupAccountSidW(
            std::ptr::null(),
            sid,
            std::ptr::null_mut(),
            &mut name_len,
            std::ptr::null_mut(),
            &mut domain_len,
            &mut sid_use,
        )
    };
    // SAFETY: GetLastError reads thread-local state immediately after LookupAccountSidW.
    let first_error = unsafe { GetLastError() };
    if first_result != 0 || name_len == 0 || first_error != ERROR_INSUFFICIENT_BUFFER {
        return None;
    }

    let mut name = vec![0u16; name_len as usize];
    let mut domain = vec![0u16; domain_len as usize];
    // SAFETY: the SID remains valid; both UTF-16 buffers are writable for their declared lengths.
    if unsafe {
        LookupAccountSidW(
            std::ptr::null(),
            sid,
            name.as_mut_ptr(),
            &mut name_len,
            domain.as_mut_ptr(),
            &mut domain_len,
            &mut sid_use,
        )
    } == 0
    {
        return None;
    }

    let name_len = name
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(name.len());
    (name_len != 0).then(|| String::from_utf16_lossy(&name[..name_len]))
}

pub(crate) fn process_runs_as_service_account_from_handle(process: &WinHandle) -> Option<bool> {
    let token_buffer = process_token_user_buffer(process)?;
    // SAFETY: successful TokenUser output starts with a valid aligned TOKEN_USER whose SID remains
    // valid while token_buffer is alive.
    let token_user = unsafe { &*token_buffer.as_ptr().cast::<TOKEN_USER>() };
    Some(sid_is_service_account(token_user.User.Sid))
}

fn sid_is_service_account(sid: PSID) -> bool {
    [WinLocalSystemSid, WinLocalServiceSid, WinNetworkServiceSid]
        .into_iter()
        .any(|sid_type| {
            // SAFETY: callers provide a valid SID and sid_type is a documented
            // WELL_KNOWN_SID_TYPE value.
            unsafe { IsWellKnownSid(sid, sid_type) != 0 }
        })
}

fn process_token_user_buffer(process: &WinHandle) -> Option<Vec<usize>> {
    let mut token = std::ptr::null_mut();
    // SAFETY: process is live, token is writable, and the requested token access is query-only.
    if unsafe { OpenProcessToken(process.raw(), TOKEN_QUERY, &mut token) } == 0 {
        return None;
    }
    let token = WinHandle::new(token);

    let mut required_bytes = 0;
    // SAFETY: token is live; a null output with zero length requests the required buffer size.
    let first_result = unsafe {
        GetTokenInformation(
            token.raw(),
            TokenUser,
            std::ptr::null_mut(),
            0,
            &mut required_bytes,
        )
    };
    // SAFETY: GetLastError reads thread-local state immediately after GetTokenInformation.
    let first_error = unsafe { GetLastError() };
    if first_result != 0 || required_bytes == 0 || first_error != ERROR_INSUFFICIENT_BUFFER {
        return None;
    }

    let word_size = std::mem::size_of::<usize>();
    let mut token_buffer = vec![0usize; (required_bytes as usize).div_ceil(word_size)];
    // SAFETY: token is live; token_buffer is aligned and writable for required_bytes.
    if unsafe {
        GetTokenInformation(
            token.raw(),
            TokenUser,
            token_buffer.as_mut_ptr().cast(),
            required_bytes,
            &mut required_bytes,
        )
    } == 0
    {
        return None;
    }

    Some(token_buffer)
}

fn process_image_path_from_handle(process: &WinHandle) -> Option<PathBuf> {
    query_process_image_path(process).ok()
}

pub(crate) fn query_process_image_path(process: &WinHandle) -> Result<PathBuf, u32> {
    let mut buffer = vec![0u16; PROCESS_IMAGE_PATH_INITIAL_BUFFER_LEN];
    loop {
        let mut len = buffer.len() as u32;
        // SAFETY: process is live, buffer supplies its full writable capacity, and len is both the
        // input capacity and writable output length.
        let ok = unsafe {
            QueryFullProcessImageNameW(
                process.raw(),
                PROCESS_NAME_WIN32,
                buffer.as_mut_ptr(),
                &mut len,
            )
        };

        if ok != 0 {
            return if len != 0 {
                Ok(PathBuf::from(OsString::from_wide(&buffer[..len as usize])))
            } else {
                Err(windows_sys::Win32::Foundation::ERROR_INVALID_DATA)
            };
        }

        // SAFETY: GetLastError reads thread-local state immediately after the failed query.
        let error = unsafe { GetLastError() };
        if error != ERROR_INSUFFICIENT_BUFFER || buffer.len() >= PROCESS_IMAGE_PATH_MAX_BUFFER_LEN {
            return Err(error);
        }

        buffer.resize((buffer.len() * 2).min(PROCESS_IMAGE_PATH_MAX_BUFFER_LEN), 0);
    }
}

pub(crate) fn process_handle_matches_executable_path(
    process: &WinHandle,
    expected_executable_path: &Path,
) -> bool {
    process_image_path_from_handle(process)
        .is_some_and(|actual| same_executable_path(&actual, expected_executable_path))
}

#[cfg(test)]
mod tests {
    #[test]
    fn usage_sampling_skips_efficiency_queries() {
        for mode in [
            ProcessResourceSampling::CpuOnly,
            ProcessResourceSampling::CpuAndMemory,
        ] {
            let efficiency = mode.efficiency_mode(
                || panic!("usage-only sampling must not query EcoQoS"),
                || panic!("usage-only sampling must not query priority"),
            );
            assert_eq!(efficiency, None);
        }
    }

    #[test]
    fn efficiency_sampling_queries_priority_only_when_eco_qos_is_enabled() {
        use std::cell::Cell;
        use windows_sys::Win32::System::Threading::NORMAL_PRIORITY_CLASS;

        for (eco_qos, priority_class, expected, expected_priority_queries) in [
            (None, 0, None, 0),
            (Some(false), 0, Some(false), 0),
            (Some(false), IDLE_PRIORITY_CLASS, Some(false), 0),
            (Some(true), 0, None, 1),
            (Some(true), IDLE_PRIORITY_CLASS, Some(true), 1),
            (Some(true), NORMAL_PRIORITY_CLASS, Some(false), 1),
        ] {
            let eco_queries = Cell::new(0);
            let priority_queries = Cell::new(0);
            let efficiency = ProcessResourceSampling::WithEfficiencyMode.efficiency_mode(
                || {
                    eco_queries.set(eco_queries.get() + 1);
                    eco_qos
                },
                || {
                    priority_queries.set(priority_queries.get() + 1);
                    priority_class
                },
            );
            assert_eq!(efficiency, expected);
            assert_eq!(eco_queries.get(), 1);
            assert_eq!(priority_queries.get(), expected_priority_queries);
        }
    }

    #[test]
    fn resource_sampling_keeps_identity_and_cpu_and_reads_selected_counters() {
        let process_id = std::process::id();
        let cpu = sample_process_resource(process_id, ProcessResourceSampling::CpuOnly)
            .expect("current-process CPU time should be readable");
        let usage = sample_process_resource(process_id, ProcessResourceSampling::CpuAndMemory)
            .expect("current-process usage should be readable");
        let full = sample_process_resource(process_id, ProcessResourceSampling::WithEfficiencyMode)
            .expect("current-process resources should be readable");

        assert_eq!(cpu.creation_time, usage.creation_time);
        assert_eq!(usage.creation_time, full.creation_time);
        assert_eq!(cpu.working_set_bytes, None);
        assert_eq!(cpu.private_commit_bytes, None);
        assert_eq!(cpu.efficiency_mode, None);
        assert!(usage.working_set_bytes.is_some() && usage.private_commit_bytes.is_some());
        assert!(full.working_set_bytes.is_some() && full.private_commit_bytes.is_some());
        assert_eq!(usage.efficiency_mode, None);
        assert!(usage.cpu.cpu_time_100ns >= cpu.cpu.cpu_time_100ns);
        assert!(usage.cpu.sampled_at >= cpu.cpu.sampled_at);
        assert!(full.cpu.cpu_time_100ns >= usage.cpu.cpu_time_100ns);
        assert!(full.cpu.sampled_at >= usage.cpu.sampled_at);
    }

    #[test]
    fn executable_properties_rejects_unavailable_paths() {
        assert!(super::open_process_properties(std::path::Path::new("")).is_err());
        assert!(super::open_process_properties(std::path::Path::new("relative.exe")).is_err());
    }

    use super::*;

    #[test]
    fn process_name_from_entry_ignores_system_process() {
        let entry = process_entry("[System Process]");

        assert_eq!(process_name_from_entry(&entry), None);
    }

    #[test]
    fn process_name_from_entry_keeps_normal_processes_lowercase() {
        let entry = process_entry("Explorer.EXE");

        assert_eq!(
            process_name_from_entry(&entry).as_deref(),
            Some("explorer.exe")
        );
    }

    #[test]
    fn process_action_target_errors_keep_typed_failures_and_win32_codes() {
        assert_eq!(
            ProcessActionTargetError::ProcessChanged.to_string(),
            "The selected process instance has changed."
        );
        assert_eq!(
            ProcessActionTargetError::ProcessUnavailable(5).to_string(),
            "The selected process is no longer available (Win32 error 5)."
        );
    }

    #[test]
    fn process_tree_postorder_covers_multiple_roots_once_and_children_first() {
        let process = |id, parent_id| ProcessInfo {
            id,
            creation_time: Some(u64::from(id)),
            parent_id,
            session_id: None,
            user_name: None,
            is_service_account: None,
            is_critical: Some(false),
            can_set_information: true,
            name: format!("process-{id}.exe"),
            image_path: Some(PathBuf::from(format!(r"C:\Apps\process-{id}.exe"))),
        };
        let processes = [
            process(10, None),
            process(11, Some(10)),
            process(20, None),
            process(21, Some(20)),
        ];
        let captured_roots = [10, 11, 20]
            .into_iter()
            .map(|process_id| {
                process_action_target_from_snapshot(
                    processes
                        .iter()
                        .find(|process| process.id == process_id)
                        .unwrap(),
                )
                .unwrap()
            })
            .collect::<Vec<_>>();

        assert_eq!(
            process_tree_ids_postorder(&[10, 11, 20], &processes).unwrap(),
            vec![11, 10, 21, 20]
        );
        let targets = process_tree_action_targets(&captured_roots, &processes).unwrap();
        assert_eq!(
            targets.iter().map(|target| target.id).collect::<Vec<_>>(),
            vec![11, 10, 21, 20]
        );
        assert_eq!(targets[0].creation_time, 11);
    }

    #[test]
    fn process_tree_rejects_a_reused_root_instance() {
        let process = |creation_time, path: &str| ProcessInfo {
            id: 10,
            creation_time: Some(creation_time),
            parent_id: None,
            session_id: Some(1),
            user_name: None,
            is_service_account: Some(false),
            is_critical: Some(false),
            can_set_information: true,
            name: Path::new(path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            image_path: Some(PathBuf::from(path)),
        };
        let captured =
            process_action_target_from_snapshot(&process(100, r"C:\Apps\original.exe")).unwrap();
        let current = [process(200, r"C:\Apps\replacement.exe")];

        let error = process_tree_action_targets(&[captured], &current).unwrap_err();

        assert!(error.contains("selected process instance changed"));
    }

    #[test]
    fn process_tree_excludes_children_older_than_a_reused_parent_pid() {
        let process = |id, parent_id, creation_time| ProcessInfo {
            id,
            creation_time: Some(creation_time),
            parent_id,
            session_id: None,
            user_name: None,
            is_service_account: None,
            is_critical: Some(false),
            can_set_information: true,
            name: format!("process-{id}.exe"),
            image_path: Some(PathBuf::from(format!(r"C:\Apps\process-{id}.exe"))),
        };
        let processes = [
            process(10, None, 100),
            process(11, Some(10), 50),
            process(12, Some(10), 100),
        ];
        let captured_root = process_action_target_from_snapshot(&processes[0]).unwrap();

        assert_eq!(
            process_tree_ids_postorder(&[10], &processes).unwrap(),
            vec![12, 10]
        );
        assert_eq!(
            process_tree_action_targets(&[captured_root], &processes)
                .unwrap()
                .into_iter()
                .map(|target| target.id)
                .collect::<Vec<_>>(),
            vec![12, 10]
        );
    }

    #[test]
    fn protected_processes_match_foreground_and_visible_apps_by_pid_or_path() {
        let process = |id, path: &str| ProcessInfo {
            id,
            creation_time: Some(u64::from(id)),
            parent_id: None,
            session_id: None,
            user_name: None,
            is_service_account: None,
            is_critical: Some(false),
            can_set_information: true,
            name: "app.exe".to_owned(),
            image_path: Some(PathBuf::from(path)),
        };
        let processes = [
            process(42, r"C:\Apps\Foreground\app.exe"),
            process(77, r"C:\Apps\Visible\app.exe"),
        ];
        let protected =
            ProtectedProcesses::capture(&processes, true, Some(42), BTreeSet::from([77]));

        assert!(protected.contains(42, Path::new(r"C:\Apps\helper.exe")));
        assert!(protected.contains(77, Path::new(r"C:\Apps\helper.exe")));
        assert!(protected.contains(99, Path::new(r"c:\apps\visible\APP.EXE")));
        assert!(!protected.contains(99, Path::new(r"D:\Other\app.exe")));

        let mut candidate = process(99, r"D:\Other\app.exe");
        candidate.image_path = None;
        assert!(protected.may_contain(&candidate));
        candidate.name = "unrelated.exe".to_owned();
        assert!(!protected.may_contain(&candidate));
        candidate.id = 77;
        assert!(protected.may_contain(&candidate));
        candidate.id = 99;
        candidate.image_path = Some(PathBuf::from(r"C:\Apps\Visible\app.exe"));
        assert!(protected.may_contain(&candidate));

        let unprotected = ProtectedProcesses::capture(&processes, false, Some(42), BTreeSet::new());
        assert!(!unprotected.contains(42, Path::new(r"C:\Apps\Foreground\app.exe")));
    }

    #[test]
    fn path_enrichment_keeps_safety_metadata_and_limits_account_names_to_ui() {
        let process_id = std::process::id();
        let mut current = list_processes_with_paths()
            .expect("process snapshot is available")
            .into_iter()
            .find(|process| process.id == process_id)
            .expect("current process is present in its own snapshot");
        assert!(current
            .user_name
            .as_ref()
            .is_some_and(|name| !name.is_empty()));
        assert!(current.creation_time.is_some());
        assert!(current.image_path.is_some());
        assert!(current.is_service_account.is_some());
        assert!(current.is_critical.is_some());

        let mut expected = current.clone();
        expected.user_name = None;
        enrich_process_paths(std::slice::from_mut(&mut current));
        assert!(
            current.user_name.is_none(),
            "runtime enrichment must omit account names"
        );
        assert_eq!(current, expected);
    }

    #[test]
    fn account_name_cache_reuses_successes_by_sid_and_retries_failures() {
        let mut names = BTreeMap::new();
        assert_eq!(cached_account_name(&mut names, &[1], || None), None);
        assert_eq!(
            cached_account_name(&mut names, &[1], || Some("Alice".to_owned())).as_deref(),
            Some("Alice")
        );
        assert_eq!(
            cached_account_name(&mut names, &[1], || panic!("SID is already cached")).as_deref(),
            Some("Alice")
        );
        assert_eq!(
            cached_account_name(&mut names, &[2], || Some("Bob".to_owned())).as_deref(),
            Some("Bob")
        );
        assert_eq!(names.len(), 2);
    }

    #[test]
    fn process_id_capture_retries_a_full_buffer_before_returning_any_ids() {
        let mut attempts = 0;
        let ids = read_process_ids(|ids, bytes| {
            attempts += 1;
            if attempts == 1 {
                assert_eq!(ids.len(), 512);
                ids[0] = 123;
                return Ok(bytes);
            }
            assert_eq!(ids.len(), 1024);
            ids[..3].copy_from_slice(&[0, 20, 40]);
            Ok(12)
        })
        .expect("the second scan fits");
        assert_eq!(attempts, 2);
        assert_eq!(ids, [0, 20, 40]);
    }

    #[test]
    fn process_id_capture_rejects_failed_or_malformed_results() {
        let mut attempts = 0;
        let result = read_process_ids(|_, bytes| {
            attempts += 1;
            if attempts == 1 {
                Ok(bytes)
            } else {
                Err("read failed".to_owned())
            }
        });
        assert_eq!(result, Err("read failed".to_owned()));
        for offset in [-1, 4] {
            assert!(read_process_ids(|_, bytes| Ok(bytes.saturating_add_signed(offset))).is_err());
        }
    }

    #[test]
    fn process_snapshot_captures_the_current_process_creation_time() {
        // SAFETY: GetCurrentProcessId takes no arguments and has no caller requirements.
        let process_id = unsafe { GetCurrentProcessId() };
        let processes = list_processes().expect("process snapshot is available");
        let current = processes
            .iter()
            .find(|process| process.id == process_id)
            .expect("current process is present in its own snapshot");

        assert!(current.creation_time.is_some());
    }

    #[test]
    fn observed_image_and_visible_siblings_require_the_captured_generation() {
        let id = std::process::id();
        let handle = open_process_for_query(id).expect("current process should be queryable");
        let creation_time = handle
            .process_creation_time()
            .expect("current process creation time should be readable");
        let path =
            query_process_image_path(&handle).expect("current image path should be readable");
        let path_text = path.to_str().expect("current image path should be Unicode");
        let mut observed = ProcessInfo {
            id,
            creation_time: Some(creation_time),
            parent_id: None,
            session_id: None,
            user_name: None,
            is_service_account: None,
            is_critical: None,
            can_set_information: false,
            name: path.file_name().unwrap().to_string_lossy().into_owned(),
            image_path: Some(PathBuf::from("cached.exe")),
        };

        assert_eq!(
            observed_process_executable_path(&observed),
            Some(path.clone())
        );
        assert_eq!(process_executable_path(&observed), None);

        let sibling_id = id ^ 1;
        let capture = |process: &ProcessInfo| {
            ProtectedProcesses::capture(
                std::slice::from_ref(process),
                false,
                None,
                BTreeSet::from([id]),
            )
        };
        observed.image_path = None;
        assert_eq!(
            process_executable_path(&observed).as_deref(),
            Some(path.as_path())
        );
        assert!(process_matches_executable_path(&observed, path_text));
        assert!(capture(&observed).contains(sibling_id, &path));

        observed.creation_time = Some(creation_time ^ 1);
        assert_eq!(observed_process_executable_path(&observed), None);
        assert_eq!(process_executable_path(&observed), None);
        assert!(!process_matches_executable_path(&observed, path_text));
        let stale = capture(&observed);
        assert!(stale.contains(id, &path));
        assert!(!stale.contains(sibling_id, &path));

        observed.creation_time = None;
        assert_eq!(observed_process_executable_path(&observed), None);
        assert_eq!(process_executable_path(&observed), None);
        assert!(!process_matches_executable_path(&observed, path_text));
        let cached_path = PathBuf::from(r"C:\Apps\cached.exe");
        observed.image_path = Some(cached_path.clone());
        assert_eq!(process_executable_path(&observed), None);
        assert!(!capture(&observed).contains(sibling_id, &cached_path));

        observed.creation_time = Some(creation_time);
        assert!(capture(&observed).contains(sibling_id, &cached_path));
    }

    #[test]
    fn service_account_sids_are_identified() {
        use windows_sys::Win32::Security::{
            CreateWellKnownSid, WinBuiltinUsersSid, SECURITY_MAX_SID_SIZE,
        };

        let make_sid = |sid_type| {
            let mut sid = vec![
                0usize;
                (SECURITY_MAX_SID_SIZE as usize)
                    .div_ceil(std::mem::size_of::<usize>())
            ];
            let mut sid_len = SECURITY_MAX_SID_SIZE;
            assert_ne!(
                // SAFETY: sid is writable for sid_len bytes, the domain SID is optional, and
                // sid_type is a documented WELL_KNOWN_SID_TYPE value.
                unsafe {
                    CreateWellKnownSid(
                        sid_type,
                        std::ptr::null_mut(),
                        sid.as_mut_ptr().cast(),
                        &mut sid_len,
                    )
                },
                0
            );
            sid
        };

        for sid_type in [WinLocalSystemSid, WinLocalServiceSid, WinNetworkServiceSid] {
            let mut sid = make_sid(sid_type);
            assert!(sid_is_service_account(sid.as_mut_ptr().cast()));
        }
        let mut users_sid = make_sid(WinBuiltinUsersSid);
        assert!(!sid_is_service_account(users_sid.as_mut_ptr().cast()));
    }

    #[test]
    fn process_action_targets_use_operation_specific_access() {
        assert_eq!(ProcessActionAccess::SafetyOnly.desired_access(), 0);
        assert_eq!(
            ProcessActionAccess::SetInformation.desired_access(),
            PROCESS_SET_INFORMATION
        );
        assert_eq!(
            ProcessActionAccess::TrimWorkingSet.desired_access(),
            PROCESS_SET_QUOTA
        );
        assert_eq!(
            ProcessActionAccess::Terminate.desired_access(),
            PROCESS_TERMINATE
        );
        assert_eq!(
            ProcessActionAccess::AssignToJob.desired_access(),
            PROCESS_SET_QUOTA | PROCESS_TERMINATE
        );
        let target = ProcessActionTarget {
            id: 42,
            name: "explorer.exe".to_owned(),
            executable_path: PathBuf::from(r"C:\Windows\explorer.exe"),
            creation_time: 1,
            session_id: Some(1),
            is_service_account: Some(false),
        };

        assert!(
            ensure_process_action_target_access(&target, ProcessActionAccess::SafetyOnly).is_err()
        );

        // SAFETY: GetCurrentProcessId takes no arguments and has no caller requirements.
        let current_process_id = unsafe { GetCurrentProcessId() };
        let target = ProcessActionTarget {
            id: current_process_id,
            name: "editor.exe".to_owned(),
            ..target
        };
        assert!(
            ensure_process_action_target_access(&target, ProcessActionAccess::SetInformation)
                .is_ok()
        );
    }

    #[test]
    fn process_candidates_exclude_critical_and_unverifiable_processes() {
        let process = ProcessInfo {
            id: 42,
            creation_time: Some(1),
            parent_id: None,
            session_id: Some(1),
            user_name: Some("User".to_owned()),
            is_service_account: Some(false),
            is_critical: Some(false),
            can_set_information: true,
            name: "editor.exe".to_owned(),
            image_path: Some(PathBuf::from(r"C:\Apps\editor.exe")),
        };
        let mut critical = process.clone();
        critical.id = 43;
        critical.name = "critical.exe".to_owned();
        critical.image_path = Some(PathBuf::from(r"C:\Windows\critical.exe"));
        critical.is_critical = Some(true);
        let mut unverifiable = process.clone();
        unverifiable.id = 44;
        unverifiable.name = "unknown.exe".to_owned();
        unverifiable.image_path = Some(PathBuf::from(r"C:\Apps\unknown.exe"));
        unverifiable.is_critical = None;
        let mut inaccessible = process.clone();
        inaccessible.id = 45;
        inaccessible.name = "protected-service.exe".to_owned();
        inaccessible.image_path = Some(PathBuf::from(r"C:\Apps\protected-service.exe"));
        inaccessible.can_set_information = false;
        let mut session_zero = process.clone();
        session_zero.id = 46;
        session_zero.session_id = Some(0);
        let mut service_account = process.clone();
        service_account.id = 47;
        service_account.is_service_account = Some(true);

        let candidates = process_candidates_from_processes(&[
            session_zero.clone(),
            process,
            critical,
            unverifiable,
            inaccessible,
        ]);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].name, "editor.exe");
        assert!(candidates[0].has_suspendable_instance);

        let candidates = process_candidates_from_processes(&[session_zero]);
        assert_eq!(candidates.len(), 1);
        assert!(!candidates[0].has_suspendable_instance);

        let candidates = process_candidates_from_processes(&[service_account]);
        assert_eq!(candidates.len(), 1);
        assert!(!candidates[0].has_suspendable_instance);
    }

    #[test]
    fn catalog_keeps_suspendability_separate_for_lookalike_paths() {
        let path = "C:\\Apps\\\u{fffd}\\editor.exe";
        let valid = ProcessInfo {
            id: 42,
            creation_time: Some(1),
            parent_id: None,
            session_id: Some(0),
            user_name: None,
            is_service_account: Some(false),
            is_critical: Some(false),
            can_set_information: true,
            name: "editor.exe".to_owned(),
            image_path: Some(path.into()),
        };
        let malformed = path
            .encode_utf16()
            .map(|unit| if unit == 0xfffd { 0xd800 } else { unit })
            .collect::<Vec<_>>();
        let raw = ProcessInfo {
            id: 43,
            session_id: Some(1),
            image_path: Some(OsString::from_wide(&malformed).into()),
            ..valid.clone()
        };

        let candidates = process_candidates_from_processes(&[raw, valid]);
        assert_eq!(candidates.len(), 2);
        assert!(candidates.iter().any(|candidate| {
            candidate.image_path == Path::new(path) && !candidate.has_suspendable_instance
        }));
    }

    #[test]
    fn windows_explorer_path_is_absolute_and_trusted() {
        let path = windows_explorer_path().expect("Windows directory is available");

        assert!(path.is_absolute());
        assert_eq!(path.file_name(), Some(std::ffi::OsStr::new("explorer.exe")));
    }

    #[test]
    fn executable_path_comparison_preserves_normalization_and_identity() {
        let path = Path::new(r"C:\Games\game.exe");
        for (other, expected) in [
            (r"C:\Games\game.exe", true),
            ("C:/Games/game.exe", true),
            ("C:\\Games\\game.exe\u{3000}", false),
            ("C:\\Games\\game.exe ", false),
            (r"c:\games\GAME.exe", true),
            (r"C:\Other\game.exe", false),
            (r"C:\Games\other.exe", false),
        ] {
            assert_eq!(
                same_executable_path(path, Path::new(other)),
                expected,
                "{other}"
            );
        }
    }

    #[test]
    fn executable_path_comparison_requires_exact_native_identity_for_non_unicode_paths() {
        let native_path = |unit| {
            let mut wide = r"C:\Games\directory-".encode_utf16().collect::<Vec<_>>();
            wide.push(unit);
            wide.extend(r"\game.exe".encode_utf16());
            PathBuf::from(OsString::from_wide(&wide))
        };
        let path = native_path(0xd800);

        assert!(same_executable_path(&path, &native_path(0xd800)));
        assert!(!same_executable_path(&path, &native_path(0xd801)));
        assert!(!same_executable_path(
            &path,
            Path::new("C:\\Games\\directory-\u{fffd}\\game.exe"),
        ));
    }

    #[test]
    fn executable_name_comparison_preserves_unicode_on_both_buffer_paths() {
        for prefix in [String::new(), "a".repeat(128)] {
            let upper = format!("{prefix}Ü\u{1f680}.EXE");
            let lower = format!("{prefix}ü\u{1f680}.exe");
            let different = format!("{prefix}ü\u{1f681}.exe");
            assert!(same_executable_name(&upper, &lower));
            assert!(!same_executable_name(&upper, &different));
        }
    }

    #[test]
    fn power_profile_keys_preserve_verbatim_path_spellings() {
        for (left, right, equivalent) in [
            (r"C:\Apps\worker.exe", "C:/Apps/worker.exe", true),
            (
                r"\\server\share\Apps\worker.exe",
                "//server/share/Apps/worker.exe",
                true,
            ),
            (r"\\?\C:\Apps\worker.exe", r"\\?\C:\Apps/worker.exe", false),
            (r"\\?\C:\Apps\worker.exe", "//?/C:/Apps/worker.exe", false),
            (
                r"\\?\UNC\server\share\Apps\worker.exe",
                r"\\?\UNC\server\share\Apps/worker.exe",
                false,
            ),
        ] {
            assert_eq!(
                power_profile_executable_path_key(left) == power_profile_executable_path_key(right),
                equivalent
            );
        }
    }

    #[test]
    fn executable_path_matching_distinguishes_same_named_binaries() {
        let process = ProcessInfo {
            id: 0,
            creation_time: Some(1),
            parent_id: None,
            session_id: None,
            user_name: None,
            is_service_account: None,
            is_critical: Some(false),
            can_set_information: true,
            name: "game.exe".to_owned(),
            image_path: Some(PathBuf::from(r"C:\Games\game.exe")),
        };

        assert!(process_matches_executable_path(
            &process,
            r"C:\Games\game.exe"
        ));
        assert!(process_matches_executable_path(
            &process,
            "C:/Games/game.exe"
        ));
        assert!(process_matches_executable_path(
            &process,
            "c:/games/GAME.exe"
        ));
        assert!(!process_matches_executable_path(
            &process,
            r"C:\Other\game.exe"
        ));
        assert!(!process_matches_executable_path(&process, "game.exe"));
        let process = ProcessInfo {
            image_path: None,
            ..process
        };
        assert!(!process_matches_executable_path(
            &process,
            r"C:\Games\game.exe"
        ));

        let unicode_path = r"C:\Games\éditeur.exe";
        let process = ProcessInfo {
            name: "Éditeur.exe".into(),
            image_path: Some(unicode_path.into()),
            ..process
        };
        assert!(process_matches_executable_path(&process, unicode_path));
    }

    #[test]
    fn process_failure_keys_preserve_exact_path_identity() {
        assert_eq!(
            process_failure_key(r"C:/Games/app.exe"),
            process_failure_key(r"C:\Games\app.exe")
        );
        assert_ne!(
            process_failure_key(r"C:\Games\APP.exe"),
            process_failure_key(r"C:\Games\app.exe")
        );
        assert_ne!(
            process_failure_key(r"C:\Games\app.exe"),
            process_failure_key(r"C:\Tools\app.exe")
        );
    }

    fn process_entry(name: &str) -> PROCESSENTRY32W {
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };

        for (index, code) in name.encode_utf16().enumerate() {
            entry.szExeFile[index] = code;
        }

        entry
    }
}
