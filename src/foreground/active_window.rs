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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForegroundProcess {
    pub id: u32,
    pub name: String,
}

impl ForegroundDetector {
    pub fn process(&self) -> Option<ForegroundProcess> {
        unsafe { foreground_process() }
    }

    pub fn process_id(&self) -> Option<u32> {
        unsafe { foreground_process_id() }
    }

    pub fn process_name(&self) -> Option<String> {
        self.process().map(|process| process.name)
    }
}

unsafe fn foreground_process() -> Option<ForegroundProcess> {
    let process_id = foreground_process_id()?;

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
    let name = Path::new(&path)
        .file_name()
        .map(|name| name.to_string_lossy().to_ascii_lowercase())?;

    Some(ForegroundProcess {
        id: process_id,
        name,
    })
}

unsafe fn foreground_process_id() -> Option<u32> {
    let window = GetForegroundWindow();
    if window.is_null() {
        return None;
    }

    let mut process_id = 0;
    GetWindowThreadProcessId(window, &mut process_id);
    (process_id != 0).then_some(process_id)
}
