use std::collections::BTreeSet;

use windows_sys::Win32::{
    Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
    System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessInfo {
    pub id: u32,
    pub parent_id: Option<u32>,
    pub name: String,
}

pub fn list_process_names() -> Result<Vec<String>, String> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err("Failed to read running process list.".to_owned());
    }

    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut names = BTreeSet::new();

    let mut has_entry = unsafe { Process32FirstW(snapshot, &mut entry) != 0 };
    while has_entry {
        if let Some(name) = process_name_from_entry(&entry) {
            names.insert(name);
        }

        has_entry = unsafe { Process32NextW(snapshot, &mut entry) != 0 };
    }

    unsafe {
        CloseHandle(snapshot);
    }

    Ok(names.into_iter().collect())
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
