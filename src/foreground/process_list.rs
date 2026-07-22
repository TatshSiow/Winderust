use std::{
    collections::BTreeMap, ffi::OsString, fmt, os::windows::ffi::OsStringExt, path::PathBuf,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessActionTarget {
    pub id: u32,
    pub name: String,
    pub creation_time: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessActionTargetError {
    ProtectedProcess,
    CurrentSessionUnavailable,
    DifferentSession,
    ProcessEnumeration(String),
    ProcessExited,
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
            Self::ProcessEnumeration(message) => formatter.write_str(message),
            Self::ProcessExited => formatter.write_str("Process exited."),
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
    expected_name: &str,
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
    let process = list_processes()
        .map_err(ProcessActionTargetError::ProcessEnumeration)?
        .into_iter()
        .find(|process| process.id == process_id)
        .ok_or(ProcessActionTargetError::ProcessExited)?;
    if !same_process_name(&process.name, expected_name) {
        return Err(ProcessActionTargetError::ProcessChanged);
    }
    // SAFETY: process_id was revalidated against the current snapshot and no inherited handle is
    // requested.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if handle.is_null() {
        // SAFETY: GetLastError has no caller requirements and is read immediately after the
        // failing OpenProcess call on this thread.
        return Err(ProcessActionTargetError::ProcessUnavailable(unsafe {
            GetLastError()
        }));
    }
    let creation_time = WinHandle::new(handle)
        .process_creation_time()
        .ok_or(ProcessActionTargetError::IdentityUnavailable)?;
    Ok(ProcessActionTarget {
        id: process_id,
        name: process.name,
        creation_time,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessCandidateInfo {
    pub id: u32,
    pub name: String,
    pub image_path: Option<PathBuf>,
}

pub fn list_process_candidates() -> Result<Vec<ProcessCandidateInfo>, String> {
    let snapshot = process_snapshot()?;
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut candidates = BTreeMap::new();

    // SAFETY: snapshot is live and entry declares its size and remains writable.
    let mut has_entry = unsafe { Process32FirstW(snapshot.raw(), &mut entry) != 0 };
    while has_entry {
        if let Some(name) = process_name_from_entry(&entry) {
            candidates.entry(name).or_insert(entry.th32ProcessID);
        }

        // SAFETY: snapshot remains live and entry remains writable for the next record.
        has_entry = unsafe { Process32NextW(snapshot.raw(), &mut entry) != 0 };
    }
    ensure_process_iteration_complete()?;

    Ok(candidates
        .into_iter()
        .map(|(name, id)| ProcessCandidateInfo {
            id,
            name,
            image_path: process_image_path(id),
        })
        .collect())
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
            });
        }

        // SAFETY: snapshot remains live and entry remains writable for the next record.
        has_entry = unsafe { Process32NextW(snapshot.raw(), &mut entry) != 0 };
    }
    ensure_process_iteration_complete()?;

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

pub fn process_names_by_id(processes: &[ProcessInfo]) -> BTreeMap<u32, String> {
    processes
        .iter()
        .map(|process| (process.id, process.name.clone()))
        .collect()
}

pub fn process_session_id(process_id: u32) -> Option<u32> {
    let mut session_id = 0;
    // SAFETY: session_id is writable and process_id is a value, not a borrowed handle.
    let ok = unsafe { ProcessIdToSessionId(process_id, &mut session_id) };
    (ok != 0).then_some(session_id)
}

pub fn is_foreground_process(
    process_id: u32,
    process_name: &str,
    foreground_process_id: Option<u32>,
    foreground_process_name: Option<&str>,
) -> bool {
    Some(process_id) == foreground_process_id
        || foreground_process_name
            .is_some_and(|foreground| same_process_name(foreground, process_name))
}

pub fn should_ignore_foreground_process(
    exclude_foreground_app: bool,
    process_id: u32,
    process_name: &str,
    foreground_process_id: Option<u32>,
    foreground_process_name: Option<&str>,
) -> bool {
    exclude_foreground_app
        && (foreground_process_id.is_some_and(|id| id == process_id)
            || foreground_process_name.is_some_and(|name| same_process_name(name, process_name)))
}

pub fn process_name_key(process_name: &str) -> String {
    process_name.trim().to_ascii_lowercase()
}

pub fn same_process_name(left: &str, right: &str) -> bool {
    left.trim().eq_ignore_ascii_case(right.trim())
}

pub fn contains_process_name<T: AsRef<str>>(list: &[T], process_name: &str) -> bool {
    list.iter()
        .any(|name| same_process_name(name.as_ref(), process_name))
}

pub fn process_failure_key(process_name: &str) -> String {
    process_name_key(process_name)
}

pub fn unique_app_names<'a>(names: impl Iterator<Item = &'a str>) -> Vec<String> {
    names
        .map(process_name_key)
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

fn process_image_path(process_id: u32) -> Option<PathBuf> {
    if process_id == 0 {
        return None;
    }

    // SAFETY: process_id came from a current snapshot and no inherited handle is requested.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if process.is_null() {
        return None;
    }

    let process = WinHandle::new(process);
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
            ProcessActionTargetError::ProcessExited.to_string(),
            "Process exited."
        );
        assert_eq!(
            ProcessActionTargetError::ProcessUnavailable(5).to_string(),
            "The selected process is no longer available (Win32 error 5)."
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
