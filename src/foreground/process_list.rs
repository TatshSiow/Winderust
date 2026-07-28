use std::{
    collections::BTreeMap,
    ffi::OsString,
    fmt,
    os::windows::ffi::OsStringExt,
    path::{Path, PathBuf},
};

use crate::win_util::WinHandle;

use windows_sys::Win32::{
    Foundation::{
        GetLastError, ERROR_INSUFFICIENT_BUFFER, ERROR_NO_MORE_FILES, INVALID_HANDLE_VALUE,
    },
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        },
        RemoteDesktop::ProcessIdToSessionId,
        Threading::{
            GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION,
        },
    },
};

const PROCESS_IMAGE_PATH_INITIAL_BUFFER_LEN: usize = 512;
const PROCESS_IMAGE_PATH_MAX_BUFFER_LEN: usize = 32_768;

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
    pub name: String,
    pub image_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessActionTarget {
    pub id: u32,
    pub name: String,
    pub executable_path: PathBuf,
    pub creation_time: u64,
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
) -> Result<ProcessActionTarget, ProcessActionTargetError> {
    // SAFETY: GetCurrentProcessId takes no arguments and has no caller requirements.
    let current_process_id = unsafe { GetCurrentProcessId() };
    if process_id == 0 || process_id == current_process_id {
        return Err(ProcessActionTargetError::ProtectedProcess);
    }
    let current_session_id = process_session_id(current_process_id)
        .ok_or(ProcessActionTargetError::CurrentSessionUnavailable)?;
    if process_session_id(process_id) != Some(current_session_id) {
        return Err(ProcessActionTargetError::DifferentSession);
    }
    if !expected_executable_path.is_absolute() {
        return Err(ProcessActionTargetError::IdentityUnavailable);
    }
    // SAFETY: process_id came from the current-session process list and no inherited handle is
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
    for process in &mut processes {
        process.image_path = process_image_path(process.id);
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
    if process_id == 0 {
        return None;
    }

    // SAFETY: process_id came from a current snapshot and no inherited handle is requested.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if process.is_null() {
        return None;
    }

    process_image_path_from_handle(&WinHandle::new(process))
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
    fn executable_path_matching_distinguishes_same_named_binaries() {
        let process = ProcessInfo {
            id: 42,
            parent_id: None,
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
