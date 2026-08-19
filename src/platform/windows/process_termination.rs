use windows_sys::Win32::System::Threading::TerminateProcess;

use crate::win_util::WinHandle;

use super::ProcessOperationError;

pub(crate) fn terminate(process: &WinHandle) -> Result<(), ProcessOperationError> {
    // SAFETY: process is a verified live handle opened with PROCESS_TERMINATE.
    let ok = unsafe { TerminateProcess(process.raw(), 1) };
    if ok == 0 {
        Err(ProcessOperationError::capture("TerminateProcess"))
    } else {
        Ok(())
    }
}
