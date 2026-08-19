use windows_sys::Win32::{
    Foundation::FILETIME,
    System::{
        ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
        Threading::{GetProcessTimes, SetProcessWorkingSetSize},
    },
};

use crate::win_util::{filetime_to_u64, WinHandle};

use super::ProcessOperationError;

pub(crate) fn working_set_bytes(process: &WinHandle) -> Result<u64, ProcessOperationError> {
    let mut counters = PROCESS_MEMORY_COUNTERS {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ..Default::default()
    };
    // SAFETY: process is a verified live handle and counters is writable for exactly the supplied
    // structure size.
    let ok = unsafe {
        K32GetProcessMemoryInfo(
            process.raw(),
            &mut counters,
            std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        )
    };
    if ok == 0 {
        Err(ProcessOperationError::capture("K32GetProcessMemoryInfo"))
    } else {
        Ok(counters.WorkingSetSize as u64)
    }
}

pub(crate) fn cpu_time_100ns(process: &WinHandle) -> Result<u64, ProcessOperationError> {
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: process is a verified live handle and every FILETIME output is writable for the
    // call.
    let ok = unsafe {
        GetProcessTimes(
            process.raw(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    };
    if ok == 0 {
        Err(ProcessOperationError::capture("GetProcessTimes"))
    } else {
        Ok(filetime_to_u64(kernel).saturating_add(filetime_to_u64(user)))
    }
}

pub(crate) fn trim(process: &WinHandle) -> Result<(), ProcessOperationError> {
    // SAFETY: process is a verified live handle with PROCESS_SET_QUOTA; both usize::MAX values
    // are the documented request to empty its working set.
    let ok = unsafe { SetProcessWorkingSetSize(process.raw(), usize::MAX, usize::MAX) };
    if ok == 0 {
        Err(ProcessOperationError::capture("SetProcessWorkingSetSize"))
    } else {
        Ok(())
    }
}
