use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fmt,
    os::windows::ffi::OsStringExt,
    path::{Path, PathBuf},
    process::Command,
    time::Instant,
};

use crate::{
    cpu::ProcessCpuSample,
    win_util::{filetime_to_u64, WinHandle},
};

use windows_sys::Win32::{
    Foundation::{
        GetLastError, ERROR_INSUFFICIENT_BUFFER, ERROR_NO_MORE_FILES, FILETIME, HANDLE,
        INVALID_HANDLE_VALUE, NTSTATUS,
    },
    Security::{
        GetTokenInformation, LookupAccountSidW, TokenUser, SID_NAME_USE, TOKEN_QUERY, TOKEN_USER,
    },
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        },
        ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
        RemoteDesktop::ProcessIdToSessionId,
        SystemInformation::GetSystemWindowsDirectoryW,
        Threading::{
            GetCurrentProcessId, GetPriorityClass, GetProcessInformation, GetProcessTimes,
            IsProcessCritical, OpenProcess, OpenProcessToken, ProcessPowerThrottling,
            ProcessProtectionLevelInfo, QueryFullProcessImageNameW, TerminateProcess,
            IDLE_PRIORITY_CLASS, PROCESS_NAME_WIN32, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE,
            PROCESS_PROTECTION_LEVEL_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_SET_INFORMATION, PROCESS_SET_QUOTA, PROCESS_TERMINATE, PROTECTION_LEVEL_NONE,
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
    pub parent_id: Option<u32>,
    pub session_id: Option<u32>,
    pub user_name: Option<String>,
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
    pub efficiency_mode: Option<bool>,
}

pub fn sample_process_resources(processes: &[ProcessInfo]) -> BTreeMap<u32, ProcessResourceSample> {
    processes
        .iter()
        .filter_map(|process| {
            sample_process_resource(process.id).map(|sample| (process.id, sample))
        })
        .collect()
}

fn sample_process_resource(process_id: u32) -> Option<ProcessResourceSample> {
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

    let mut memory = PROCESS_MEMORY_COUNTERS {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ..Default::default()
    };
    // SAFETY: handle is live and memory is writable for the supplied structure size.
    let working_set_bytes = (unsafe {
        K32GetProcessMemoryInfo(
            handle.raw(),
            &mut memory,
            std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        )
    } != 0)
        .then_some(memory.WorkingSetSize as u64);

    let mut power_throttling = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ..Default::default()
    };
    // SAFETY: handle is live and power_throttling is writable for the supplied structure size.
    let eco_qos = (unsafe {
        GetProcessInformation(
            handle.raw(),
            ProcessPowerThrottling,
            (&mut power_throttling as *mut PROCESS_POWER_THROTTLING_STATE).cast(),
            std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        )
    } != 0)
        .then_some(
            power_throttling.ControlMask & PROCESS_POWER_THROTTLING_EXECUTION_SPEED != 0
                && power_throttling.StateMask & PROCESS_POWER_THROTTLING_EXECUTION_SPEED != 0,
        );
    // SAFETY: handle is live and GetPriorityClass only reads process state.
    let priority_class = unsafe { GetPriorityClass(handle.raw()) };
    let efficiency_mode = eco_qos
        .filter(|enabled| !enabled || priority_class != 0)
        .map(|enabled| enabled && priority_class == IDLE_PRIORITY_CLASS);

    Some(ProcessResourceSample {
        cpu: ProcessCpuSample {
            cpu_time_100ns: filetime_to_u64(kernel).saturating_add(filetime_to_u64(user)),
            sampled_at: Instant::now(),
        },
        creation_time: filetime_to_u64(creation),
        working_set_bytes,
        efficiency_mode,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessActionTarget {
    pub id: u32,
    pub name: String,
    pub executable_path: PathBuf,
    pub creation_time: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProcessActionAccess {
    SafetyOnly,
    SetInformation,
    Terminate,
    AssignToJob,
}

impl ProcessActionAccess {
    fn desired_access(self) -> u32 {
        match self {
            Self::SafetyOnly => 0,
            Self::SetInformation => PROCESS_SET_INFORMATION,
            Self::Terminate => PROCESS_TERMINATE,
            Self::AssignToJob => PROCESS_SET_QUOTA | PROCESS_TERMINATE,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessActionTargetError {
    ProtectedProcess,
    CurrentSessionUnavailable,
    DifferentSession,
    ProcessChanged,
    ProcessUnavailable(u32),
    IdentityUnavailable,
}

impl fmt::Display for ProcessActionTargetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProtectedProcess => formatter.write_str("Winderust cannot modify this process."),
            Self::CurrentSessionUnavailable => {
                formatter.write_str("Could not determine the current Windows session.")
            }
            Self::DifferentSession => {
                formatter.write_str("Processes in another Windows session cannot be modified.")
            }
            Self::ProcessChanged => {
                formatter.write_str("The selected process instance has changed.")
            }
            Self::ProcessUnavailable(error) => write!(
                formatter,
                "The selected process is no longer available (Win32 error {error})."
            ),
            Self::IdentityUnavailable => {
                formatter.write_str("Could not identify the selected process instance.")
            }
        }
    }
}

impl std::error::Error for ProcessActionTargetError {}

pub fn capture_process_action_target(
    process_id: u32,
    expected_executable_path: &Path,
    allow_cross_session: bool,
) -> Result<ProcessActionTarget, ProcessActionTargetError> {
    // SAFETY: GetCurrentProcessId takes no arguments and has no caller requirements.
    let current_process_id = unsafe { GetCurrentProcessId() };
    if process_id == 0 || process_id == current_process_id {
        return Err(ProcessActionTargetError::ProtectedProcess);
    }
    if !allow_cross_session {
        let current_session_id = process_session_id(current_process_id)
            .ok_or(ProcessActionTargetError::CurrentSessionUnavailable)?;
        if process_session_id(process_id) != Some(current_session_id) {
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
    if process_critical_from_handle(&process) != Some(false) {
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
    Ok(ProcessActionTarget {
        id: process_id,
        name,
        executable_path,
        creation_time,
    })
}

pub fn terminate_process(target: &ProcessActionTarget) -> Result<(), String> {
    ensure_process_action_target_access(target, ProcessActionAccess::Terminate)?;
    let process = open_verified_action_process(target, PROCESS_TERMINATE)?;
    // SAFETY: process is a verified live handle opened with PROCESS_TERMINATE.
    if unsafe { TerminateProcess(process.raw(), 1) } == 0 {
        // SAFETY: GetLastError has no caller requirements and is read immediately after the
        // failing TerminateProcess call on this thread.
        let error = unsafe { GetLastError() };
        Err(format!("TerminateProcess failed with error {error}."))
    } else {
        Ok(())
    }
}

pub fn terminate_process_tree(
    root: &ProcessActionTarget,
    processes: &[ProcessInfo],
    allow_cross_session: bool,
) -> Result<usize, String> {
    ensure_process_action_target_access(root, ProcessActionAccess::Terminate)?;
    let mut process_ids = vec![root.id];
    let mut selected = BTreeSet::from([root.id]);
    let mut index = 0;
    while index < process_ids.len() {
        let parent_id = process_ids[index];
        for process in processes {
            if process.parent_id == Some(parent_id) && selected.insert(process.id) {
                process_ids.push(process.id);
            }
        }
        index += 1;
    }

    let mut targets = Vec::with_capacity(process_ids.len());
    for process_id in process_ids.into_iter().rev() {
        if process_id == root.id {
            targets.push(root.clone());
            continue;
        }
        let process = processes
            .iter()
            .find(|process| process.id == process_id)
            .ok_or_else(|| "A child process exited before it could be stopped.".to_owned())?;
        let path = process
            .image_path
            .as_deref()
            .ok_or_else(|| "A child process could not be identified safely.".to_owned())?;
        targets.push(
            capture_process_action_target(process_id, path, allow_cross_session)
                .map_err(|error| error.to_string())?,
        );
    }
    let count = targets.len();
    for target in &targets {
        ensure_process_action_target_access(target, ProcessActionAccess::Terminate)?;
    }
    for target in targets {
        terminate_process(&target)?;
    }
    Ok(count)
}

pub(crate) fn ensure_process_action_target_access(
    target: &ProcessActionTarget,
    access: ProcessActionAccess,
) -> Result<(), String> {
    if contains_process_name(CORE_BUILT_IN_PROCESS_EXCLUSIONS, &target.name) {
        return Err("Built-in Windows processes cannot be modified.".to_owned());
    }
    let process = open_process_for_query(target.id)
        .ok_or_else(|| "The process is no longer accessible.".to_owned())?;
    if process_critical_from_handle(&process) != Some(false) {
        return Err("Critical or unverifiable processes cannot be modified.".to_owned());
    }
    if process_protection_from_handle(&process) != Some(false) {
        return Err("Windows protected processes cannot be modified.".to_owned());
    }
    let desired_access = access.desired_access();
    if desired_access != 0 && !process_has_access(target.id, desired_access) {
        return Err("Windows denied process control access.".to_owned());
    }
    Ok(())
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

fn open_verified_action_process(
    target: &ProcessActionTarget,
    access: u32,
) -> Result<WinHandle, String> {
    // SAFETY: target was captured from the current-session process list and the handle is not inherited.
    let handle = unsafe { OpenProcess(access | PROCESS_QUERY_LIMITED_INFORMATION, 0, target.id) };
    if handle.is_null() {
        // SAFETY: GetLastError has no caller requirements and is read immediately after the
        // failing OpenProcess call on this thread.
        let error = unsafe { GetLastError() };
        return Err(format!("OpenProcess failed with error {error}."));
    }
    let handle = WinHandle::new(handle);
    if handle.process_creation_time() != Some(target.creation_time)
        || !process_handle_matches_executable_path(&handle, &target.executable_path)
    {
        return Err("The selected process instance has changed.".to_owned());
    }
    Ok(handle)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessCandidateInfo {
    pub name: String,
    pub image_path: PathBuf,
}

pub fn list_process_candidates() -> Result<Vec<ProcessCandidateInfo>, String> {
    Ok(process_candidates_from_processes(
        &list_processes_with_paths()?,
    ))
}

pub fn process_candidates_from_processes(processes: &[ProcessInfo]) -> Vec<ProcessCandidateInfo> {
    let mut candidates = BTreeMap::new();
    for process in processes {
        if process.is_critical != Some(false) || !process.can_set_information {
            continue;
        }
        let Some(image_path) = process.image_path.as_ref() else {
            continue;
        };
        candidates
            .entry(executable_path_key(image_path))
            .or_insert(ProcessCandidateInfo {
                name: process.name.clone(),
                image_path: image_path.clone(),
            });
    }
    candidates.into_values().collect()
}

pub fn list_processes() -> Result<Vec<ProcessInfo>, String> {
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
                parent_id: (entry.th32ParentProcessID != 0).then_some(entry.th32ParentProcessID),
                session_id: process_session_id(entry.th32ProcessID),
                user_name: None,
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

    for process in &mut processes {
        let Some(handle) = open_process_for_query(process.id) else {
            continue;
        };
        process.is_critical = process_critical_from_handle(&handle);
        process.can_set_information = process_protection_from_handle(&handle) == Some(false)
            && process_has_access(process.id, PROCESS_SET_INFORMATION);
    }
    Ok(processes)
}

pub fn list_processes_with_paths() -> Result<Vec<ProcessInfo>, String> {
    let mut processes = list_processes()?;
    for process in &mut processes {
        let Some(handle) = open_process_for_query(process.id) else {
            continue;
        };
        process.image_path = process_image_path_from_handle(&handle);
        process.user_name = process_user_name_from_handle(&handle);
        process.is_critical = process_critical_from_handle(&handle);
    }
    Ok(processes)
}

pub fn for_each_process_id(mut visit: impl FnMut(u32)) -> Result<(), String> {
    let snapshot = process_snapshot()?;
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };

    // SAFETY: snapshot is live and entry declares its size and remains writable.
    let mut has_entry = unsafe { Process32FirstW(snapshot.raw(), &mut entry) != 0 };
    while has_entry {
        visit(entry.th32ProcessID);
        // SAFETY: snapshot remains live and entry remains writable for the next record.
        has_entry = unsafe { Process32NextW(snapshot.raw(), &mut entry) != 0 };
    }
    ensure_process_iteration_complete()?;

    Ok(())
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

pub fn same_process_name(left: &str, right: &str) -> bool {
    left.trim().eq_ignore_ascii_case(right.trim())
}

pub fn executable_path_key(path: &Path) -> String {
    path.to_string_lossy()
        .trim()
        .replace('/', std::path::MAIN_SEPARATOR_STR)
}

pub fn same_executable_path(left: &Path, right: &Path) -> bool {
    let left_key = executable_path_key(left);
    let right_key = executable_path_key(right);
    if left_key == right_key {
        return true;
    }
    if !left_key.eq_ignore_ascii_case(&right_key) {
        return false;
    }

    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => executable_path_key(&left) == executable_path_key(&right),
        (Err(_), Err(_)) => true,
        _ => false,
    }
}

pub fn process_matches_executable_path(process: &ProcessInfo, executable_path: &str) -> bool {
    let executable_path = Path::new(executable_path.trim());
    let Some(file_name) = executable_path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if !same_process_name(&process.name, file_name) {
        return false;
    }

    process_executable_path(process)
        .is_some_and(|actual_path| same_executable_path(&actual_path, executable_path))
}

pub fn process_executable_path(process: &ProcessInfo) -> Option<PathBuf> {
    process
        .image_path
        .clone()
        .or_else(|| process_image_path(process.id))
}

pub fn contains_process_name<T: AsRef<str>>(list: &[T], process_name: &str) -> bool {
    list.iter()
        .any(|name| same_process_name(name.as_ref(), process_name))
}

pub fn process_failure_key(process_identity: &str) -> String {
    executable_path_key(Path::new(process_identity))
}

pub fn unique_app_names<'a>(names: impl Iterator<Item = &'a str>) -> Vec<String> {
    names
        .map(|name| name.trim().to_ascii_lowercase())
        .filter(|name| !name.is_empty())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
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

pub(crate) fn process_image_path(process_id: u32) -> Option<PathBuf> {
    let process = open_process_for_query(process_id)?;
    process_image_path_from_handle(&process)
}

fn open_process_for_query(process_id: u32) -> Option<WinHandle> {
    if process_id == 0 {
        return None;
    }

    // SAFETY: process_id came from a current snapshot and no inherited handle is requested.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    (!process.is_null()).then(|| WinHandle::new(process))
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

pub fn process_is_critical(process_id: u32) -> Option<bool> {
    let process = open_process_for_query(process_id)?;
    process_critical_from_handle(&process)
}

fn process_critical_from_handle(process: &WinHandle) -> Option<bool> {
    let mut critical = 0;
    // SAFETY: process is live with query access and critical is writable for the BOOL result.
    (unsafe { IsProcessCritical(process.raw(), &mut critical) } != 0).then_some(critical != 0)
}

fn process_user_name_from_handle(process: &WinHandle) -> Option<String> {
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

    // SAFETY: successful TokenUser output starts with a valid aligned TOKEN_USER whose SID remains
    // valid while token_buffer is alive.
    let token_user = unsafe { &*token_buffer.as_ptr().cast::<TOKEN_USER>() };
    let mut name_len = 0;
    let mut domain_len = 0;
    let mut sid_use: SID_NAME_USE = 0;
    // SAFETY: the SID comes from the live TokenUser buffer; null outputs request required lengths.
    let first_result = unsafe {
        LookupAccountSidW(
            std::ptr::null(),
            token_user.User.Sid,
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
            token_user.User.Sid,
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

fn process_image_path_from_handle(process: &WinHandle) -> Option<PathBuf> {
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
            return (len != 0).then(|| PathBuf::from(OsString::from_wide(&buffer[..len as usize])));
        }

        // SAFETY: GetLastError reads thread-local state immediately after the failed query.
        if unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER
            || buffer.len() >= PROCESS_IMAGE_PATH_MAX_BUFFER_LEN
        {
            return None;
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
    fn current_process_account_name_is_available() {
        // SAFETY: GetCurrentProcessId takes no arguments and has no caller requirements.
        let process_id = unsafe { GetCurrentProcessId() };
        let process = open_process_for_query(process_id).expect("current process is queryable");
        let user_name =
            process_user_name_from_handle(&process).expect("current process token resolves");

        assert!(!user_name.is_empty());
    }

    #[test]
    fn process_action_targets_use_operation_specific_access() {
        assert_eq!(ProcessActionAccess::SafetyOnly.desired_access(), 0);
        assert_eq!(
            ProcessActionAccess::SetInformation.desired_access(),
            PROCESS_SET_INFORMATION
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
            parent_id: None,
            session_id: Some(1),
            user_name: Some("User".to_owned()),
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

        let candidates =
            process_candidates_from_processes(&[process, critical, unverifiable, inaccessible]);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].name, "editor.exe");
    }

    #[test]
    fn windows_explorer_path_is_absolute_and_trusted() {
        let path = windows_explorer_path().expect("Windows directory is available");

        assert!(path.is_absolute());
        assert_eq!(path.file_name(), Some(std::ffi::OsStr::new("explorer.exe")));
    }

    #[test]
    fn executable_path_matching_distinguishes_same_named_binaries() {
        let process = ProcessInfo {
            id: 42,
            parent_id: None,
            session_id: None,
            user_name: None,
            is_critical: Some(false),
            can_set_information: true,
            name: "game.exe".to_owned(),
            image_path: Some(PathBuf::from(r"C:\Games\game.exe")),
        };

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
