use windows_sys::Win32::Foundation::HANDLE;

use crate::win_util::WinHandle;

const PROCESS_IO_PRIORITY: u32 = 33;
const STATUS_PROCESS_IS_TERMINATING: u32 = 0xC000010A;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IoPriorityError {
    ProcessExited,
    NtStatus(u32),
}

pub(crate) fn query(process: &WinHandle) -> Result<u32, IoPriorityError> {
    let mut priority = 0_u32;
    // SAFETY: process is a verified live handle, priority is writable for exactly the supplied
    // size, and no return-length pointer is requested.
    let status = unsafe {
        NtQueryInformationProcess(
            process.raw(),
            PROCESS_IO_PRIORITY,
            (&mut priority as *mut u32).cast(),
            std::mem::size_of::<u32>() as u32,
            std::ptr::null_mut(),
        )
    };
    status_result(status)?;
    Ok(priority)
}

pub(crate) fn set(process: &WinHandle, priority: u32) -> Result<(), IoPriorityError> {
    let mut priority = priority;
    // SAFETY: process is a verified live handle and priority points to exactly the supplied u32
    // size required by process information class 33.
    let status = unsafe {
        NtSetInformationProcess(
            process.raw(),
            PROCESS_IO_PRIORITY,
            (&mut priority as *mut u32).cast(),
            std::mem::size_of::<u32>() as u32,
        )
    };
    status_result(status)
}

fn status_result(status: i32) -> Result<(), IoPriorityError> {
    if status >= 0 {
        Ok(())
    } else if status as u32 == STATUS_PROCESS_IS_TERMINATING {
        Err(IoPriorityError::ProcessExited)
    } else {
        Err(IoPriorityError::NtStatus(status as u32))
    }
}

unsafe extern "system" {
    fn NtQueryInformationProcess(
        ProcessHandle: HANDLE,
        ProcessInformationClass: u32,
        ProcessInformation: *mut std::ffi::c_void,
        ProcessInformationLength: u32,
        ReturnLength: *mut u32,
    ) -> i32;

    fn NtSetInformationProcess(
        ProcessHandle: HANDLE,
        ProcessInformationClass: u32,
        ProcessInformation: *mut std::ffi::c_void,
        ProcessInformationLength: u32,
    ) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_classifies_exit_and_other_failures() {
        assert_eq!(status_result(0), Ok(()));
        assert_eq!(
            status_result(STATUS_PROCESS_IS_TERMINATING as i32),
            Err(IoPriorityError::ProcessExited)
        );
        assert_eq!(
            status_result(0xC000_0001u32 as i32),
            Err(IoPriorityError::NtStatus(0xC000_0001))
        );
    }
}
