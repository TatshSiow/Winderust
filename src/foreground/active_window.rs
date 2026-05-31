use std::path::Path;

use windows_sys::Win32::{
    Foundation::{CloseHandle, MAX_PATH},
    System::{
        ProcessStatus::K32GetModuleFileNameExW,
        Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
    },
    UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
};

#[derive(Debug, Default)]
pub struct ForegroundDetector;

impl ForegroundDetector {
    pub fn process_name(&self) -> Option<String> {
        unsafe { foreground_process_name() }
    }
}

unsafe fn foreground_process_name() -> Option<String> {
    let window = GetForegroundWindow();
    if window.is_null() {
        return None;
    }

    let mut process_id = 0;
    GetWindowThreadProcessId(window, &mut process_id);
    if process_id == 0 {
        return None;
    }

    let process = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, process_id);
    if process.is_null() {
        return None;
    }

    let mut buffer = [0u16; MAX_PATH as usize];
    let len = K32GetModuleFileNameExW(
        process,
        std::ptr::null_mut(),
        buffer.as_mut_ptr(),
        buffer.len() as u32,
    );
    CloseHandle(process);

    if len == 0 {
        return None;
    }

    let path = String::from_utf16_lossy(&buffer[..len as usize]);
    Path::new(&path)
        .file_name()
        .map(|name| name.to_string_lossy().to_ascii_lowercase())
}
