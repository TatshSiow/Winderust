use windows_sys::Wdk::Graphics::Direct3D::{
    D3DKMTGetProcessSchedulingPriorityClass, D3DKMTSetProcessSchedulingPriorityClass,
    D3DKMT_SCHEDULINGPRIORITYCLASS,
};

use crate::win_util::WinHandle;

const STATUS_PROCESS_IS_TERMINATING: u32 = 0xC000010A;
const STATUS_INVALID_PARAMETER: u32 = 0xC000000D;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GpuPriorityError {
    ProcessExited,
    ContextUnavailable,
    InvalidReturnedPriority(i32),
    PriorityOutOfRange(u32),
    NtStatus(u32),
}

pub(crate) fn query(process: &WinHandle) -> Result<u32, GpuPriorityError> {
    let mut priority: D3DKMT_SCHEDULINGPRIORITYCLASS = 0;
    // SAFETY: process is a verified live handle and priority is writable for the call.
    let status =
        unsafe { D3DKMTGetProcessSchedulingPriorityClass(process.raw(), &mut priority as *mut _) };
    status_result(status)?;
    u32::try_from(priority).map_err(|_| GpuPriorityError::InvalidReturnedPriority(priority))
}

pub(crate) fn set(process: &WinHandle, priority: u32) -> Result<(), GpuPriorityError> {
    let priority = D3DKMT_SCHEDULINGPRIORITYCLASS::try_from(priority)
        .map_err(|_| GpuPriorityError::PriorityOutOfRange(priority))?;
    // SAFETY: process is a verified live handle and priority fits the WDK enum representation.
    let status = unsafe { D3DKMTSetProcessSchedulingPriorityClass(process.raw(), priority) };
    status_result(status)
}

fn status_result(status: i32) -> Result<(), GpuPriorityError> {
    if status >= 0 {
        Ok(())
    } else if status as u32 == STATUS_PROCESS_IS_TERMINATING {
        Err(GpuPriorityError::ProcessExited)
    } else if status as u32 == STATUS_INVALID_PARAMETER {
        Err(GpuPriorityError::ContextUnavailable)
    } else {
        Err(GpuPriorityError::NtStatus(status as u32))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_classifies_exit_context_unavailable_and_other_failures() {
        assert_eq!(status_result(0), Ok(()));
        assert_eq!(
            status_result(STATUS_PROCESS_IS_TERMINATING as i32),
            Err(GpuPriorityError::ProcessExited)
        );
        assert_eq!(
            status_result(STATUS_INVALID_PARAMETER as i32),
            Err(GpuPriorityError::ContextUnavailable)
        );
        assert_eq!(
            status_result(0xC000_0001u32 as i32),
            Err(GpuPriorityError::NtStatus(0xC000_0001))
        );
    }
}
