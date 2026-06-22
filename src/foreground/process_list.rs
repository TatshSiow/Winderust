use std::{collections::BTreeMap, ffi::OsString, os::windows::ffi::OsStringExt, path::PathBuf};

use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, ERROR_INSUFFICIENT_BUFFER, INVALID_HANDLE_VALUE},
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        },
        RemoteDesktop::ProcessIdToSessionId,
        Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION,
        },
    },
};

const PROCESS_IMAGE_PATH_INITIAL_BUFFER_LEN: usize = 512;
const PROCESS_IMAGE_PATH_MAX_BUFFER_LEN: usize = 32_768;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessInfo {
    pub id: u32,
    pub parent_id: Option<u32>,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessCandidateInfo {
    pub id: u32,
    pub name: String,
    pub image_path: Option<PathBuf>,
}

pub fn list_process_candidates() -> Result<Vec<ProcessCandidateInfo>, String> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err("Failed to read running process list.".to_owned());
    }

    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut candidates = BTreeMap::new();

    let mut has_entry = unsafe { Process32FirstW(snapshot, &mut entry) != 0 };
    while has_entry {
        if let Some(name) = process_name_from_entry(&entry) {
            candidates.entry(name).or_insert(entry.th32ProcessID);
        }

        has_entry = unsafe { Process32NextW(snapshot, &mut entry) != 0 };
    }

    unsafe {
        CloseHandle(snapshot);
    }

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
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err("Failed to read running process list.".to_owned());
    }

    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut processes = Vec::new();

    let mut has_entry = unsafe { Process32FirstW(snapshot, &mut entry) != 0 };
    while has_entry {
        if let Some(name) = process_name_from_entry(&entry) {
            processes.push(ProcessInfo {
                id: entry.th32ProcessID,
                parent_id: (entry.th32ParentProcessID != 0).then_some(entry.th32ParentProcessID),
                name,
            });
        }

        has_entry = unsafe { Process32NextW(snapshot, &mut entry) != 0 };
    }

    unsafe {
        CloseHandle(snapshot);
    }

    Ok(processes)
}

pub fn process_names_by_id(processes: &[ProcessInfo]) -> BTreeMap<u32, String> {
    processes
        .iter()
        .map(|process| (process.id, process.name.clone()))
        .collect()
}

pub fn process_session_id(process_id: u32) -> Option<u32> {
    let mut session_id = 0;
    let ok = unsafe { ProcessIdToSessionId(process_id, &mut session_id) };
    (ok != 0).then_some(session_id)
}

pub fn process_id_matches_name(
    current_process_names: Option<&BTreeMap<u32, String>>,
    process_id: u32,
    process_name: &str,
) -> bool {
    current_process_names.is_none_or(|names| {
        names
            .get(&process_id)
            .is_some_and(|name| same_process_name(name, process_name))
    })
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

pub fn is_process_exited_message(message: &str) -> bool {
    message
        .trim()
        .trim_end_matches('.')
        .eq_ignore_ascii_case("Process exited")
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

fn is_system_process_name(name: &str) -> bool {
    name.trim().eq_ignore_ascii_case("[system process]")
}

fn process_image_path(process_id: u32) -> Option<PathBuf> {
    if process_id == 0 {
        return None;
    }

    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if process.is_null() {
        return None;
    }

    let mut buffer = vec![0u16; PROCESS_IMAGE_PATH_INITIAL_BUFFER_LEN];
    loop {
        let mut len = buffer.len() as u32;
        let ok = unsafe {
            QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buffer.as_mut_ptr(), &mut len)
        };

        if ok != 0 {
            unsafe {
                CloseHandle(process);
            }
            return (len != 0).then(|| PathBuf::from(OsString::from_wide(&buffer[..len as usize])));
        }

        if unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER
            || buffer.len() >= PROCESS_IMAGE_PATH_MAX_BUFFER_LEN
        {
            unsafe {
                CloseHandle(process);
            }
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
