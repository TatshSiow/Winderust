use std::collections::BTreeSet;

use windows_sys::Win32::{
    Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
    System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    },
};

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

fn process_name_from_entry(entry: &PROCESSENTRY32W) -> Option<String> {
    let len = entry
        .szExeFile
        .iter()
        .position(|code| *code == 0)
        .unwrap_or(entry.szExeFile.len());
    if len == 0 {
        return None;
    }

    Some(String::from_utf16_lossy(&entry.szExeFile[..len]).to_ascii_lowercase())
}
