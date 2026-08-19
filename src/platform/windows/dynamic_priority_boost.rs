use windows_sys::Win32::System::Threading::{GetProcessPriorityBoost, SetProcessPriorityBoost};

use crate::win_util::WinHandle;

use super::ProcessOperationError;

pub(crate) fn query_disabled(process: &WinHandle) -> Result<bool, ProcessOperationError> {
    let mut disabled = 0_i32;
    // SAFETY: process is a verified live handle and disabled is writable for the call.
    if unsafe { GetProcessPriorityBoost(process.raw(), &mut disabled) } == 0 {
        Err(ProcessOperationError::capture("GetProcessPriorityBoost"))
    } else {
        Ok(disabled != 0)
    }
}

pub(crate) fn set_disabled(
    process: &WinHandle,
    disabled: bool,
) -> Result<(), ProcessOperationError> {
    // SAFETY: process is a verified live handle and disabled is converted to the documented BOOL
    // representation expected by SetProcessPriorityBoost.
    if unsafe { SetProcessPriorityBoost(process.raw(), i32::from(disabled)) } == 0 {
        Err(ProcessOperationError::capture("SetProcessPriorityBoost"))
    } else {
        Ok(())
    }
}
