use windows_sys::Win32::{
    Foundation::{ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER},
    System::Threading::{
        GetCurrentProcessId, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_SET_INFORMATION, PROCESS_SET_QUOTA, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
    },
};

use crate::win_util::{last_error, WinHandle};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProcessAccess {
    SetInformation,
    SafetyOnly,
    WorkingSetTrim,
    Termination,
    Suspension,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProcessOpenError {
    AccessDenied,
    ProcessExited,
    Failed { code: u32 },
}

pub(crate) fn current_process_id() -> u32 {
    // SAFETY: GetCurrentProcessId takes no arguments and has no caller requirements.
    unsafe { GetCurrentProcessId() }
}

pub(crate) fn open(process_id: u32, access: ProcessAccess) -> Result<WinHandle, ProcessOpenError> {
    let mut last_open_error = ERROR_ACCESS_DENIED;
    for desired_access in desired_access_masks(access) {
        // SAFETY: process_id came from a captured process observation or typed command target and
        // the returned handle is not inheritable.
        let handle = unsafe { OpenProcess(*desired_access, 0, process_id) };
        if !handle.is_null() {
            return Ok(WinHandle::new(handle));
        }
        last_open_error = last_error();
    }

    match last_open_error {
        ERROR_ACCESS_DENIED => Err(ProcessOpenError::AccessDenied),
        ERROR_INVALID_PARAMETER => Err(ProcessOpenError::ProcessExited),
        code => Err(ProcessOpenError::Failed { code }),
    }
}

fn desired_access_masks(access: ProcessAccess) -> &'static [u32] {
    const SET_INFORMATION: &[u32] = &[PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_INFORMATION];
    const SAFETY_ONLY: &[u32] = &[PROCESS_QUERY_LIMITED_INFORMATION];
    const WORKING_SET_TRIM: &[u32] = &[PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_QUOTA];
    const TERMINATION: &[u32] = &[PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE];
    const SUSPENSION: &[u32] = &[
        PROCESS_QUERY_LIMITED_INFORMATION
            | PROCESS_SET_QUOTA
            | PROCESS_TERMINATE
            | PROCESS_SYNCHRONIZE,
        PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_QUOTA | PROCESS_TERMINATE,
    ];

    match access {
        ProcessAccess::SetInformation => SET_INFORMATION,
        ProcessAccess::SafetyOnly => SAFETY_ONLY,
        ProcessAccess::WorkingSetTrim => WORKING_SET_TRIM,
        ProcessAccess::Termination => TERMINATION,
        ProcessAccess::Suspension => SUSPENSION,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_access_profiles_keep_the_existing_minimal_rights() {
        assert_eq!(
            desired_access_masks(ProcessAccess::SetInformation),
            &[PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_INFORMATION]
        );
        assert_eq!(
            desired_access_masks(ProcessAccess::SafetyOnly),
            &[PROCESS_QUERY_LIMITED_INFORMATION]
        );
        assert_eq!(
            desired_access_masks(ProcessAccess::WorkingSetTrim),
            &[PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_QUOTA]
        );
        assert_eq!(
            desired_access_masks(ProcessAccess::Termination),
            &[PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE]
        );
    }

    #[test]
    fn suspension_prefers_synchronize_but_retains_the_existing_fallback() {
        assert_eq!(desired_access_masks(ProcessAccess::Suspension).len(), 2);
        assert_ne!(
            desired_access_masks(ProcessAccess::Suspension)[0] & PROCESS_SYNCHRONIZE,
            0
        );
        assert_eq!(
            desired_access_masks(ProcessAccess::Suspension)[1] & PROCESS_SYNCHRONIZE,
            0
        );
    }
}
