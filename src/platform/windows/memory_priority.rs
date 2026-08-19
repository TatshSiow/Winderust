use windows_sys::Win32::System::Threading::{
    GetProcessInformation, ProcessMemoryPriority as ProcessMemoryPriorityClass,
    SetProcessInformation, MEMORY_PRIORITY_BELOW_NORMAL, MEMORY_PRIORITY_INFORMATION,
    MEMORY_PRIORITY_LOW, MEMORY_PRIORITY_MEDIUM, MEMORY_PRIORITY_NORMAL, MEMORY_PRIORITY_VERY_LOW,
};

use crate::win_util::WinHandle;

use super::ProcessOperationError;

pub(crate) const VERY_LOW: u32 = MEMORY_PRIORITY_VERY_LOW;
pub(crate) const LOW: u32 = MEMORY_PRIORITY_LOW;
pub(crate) const MEDIUM: u32 = MEMORY_PRIORITY_MEDIUM;
pub(crate) const BELOW_NORMAL: u32 = MEMORY_PRIORITY_BELOW_NORMAL;
pub(crate) const NORMAL: u32 = MEMORY_PRIORITY_NORMAL;

pub(crate) fn query(process: &WinHandle) -> Result<u32, ProcessOperationError> {
    let mut priority = MEMORY_PRIORITY_INFORMATION::default();
    // SAFETY: process is a verified live handle and priority is writable for exactly the supplied
    // structure size.
    let ok = unsafe {
        GetProcessInformation(
            process.raw(),
            ProcessMemoryPriorityClass,
            (&mut priority as *mut MEMORY_PRIORITY_INFORMATION).cast(),
            std::mem::size_of::<MEMORY_PRIORITY_INFORMATION>() as u32,
        )
    };
    if ok == 0 {
        Err(ProcessOperationError::capture("GetProcessInformation"))
    } else {
        Ok(priority.MemoryPriority)
    }
}

pub(crate) fn set(process: &WinHandle, priority: u32) -> Result<(), ProcessOperationError> {
    let info = MEMORY_PRIORITY_INFORMATION {
        MemoryPriority: priority,
    };
    // SAFETY: process is a verified live handle and info is initialized for exactly the supplied
    // structure size.
    let ok = unsafe {
        SetProcessInformation(
            process.raw(),
            ProcessMemoryPriorityClass,
            (&info as *const MEMORY_PRIORITY_INFORMATION).cast(),
            std::mem::size_of::<MEMORY_PRIORITY_INFORMATION>() as u32,
        )
    };
    if ok == 0 {
        Err(ProcessOperationError::capture("SetProcessInformation"))
    } else {
        Ok(())
    }
}
