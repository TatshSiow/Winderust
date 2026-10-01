use std::{ptr::null_mut, sync::OnceLock};

use windows_sys::Win32::{
    Foundation::{ERROR_NOT_ALL_ASSIGNED, HANDLE, LUID},
    Security::{
        AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES,
        SE_INC_BASE_PRIORITY_NAME, SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

use crate::win_util::{last_error, WinHandle};

const PROCESS_IO_PRIORITY: u32 = 33;
const STATUS_PROCESS_IS_TERMINATING: u32 = 0xC000010A;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IoPriorityError {
    ProcessExited,
    PrivilegeSetup(u32),
    NtStatus(u32),
}

impl std::fmt::Display for IoPriorityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ProcessExited => f.write_str("Process exited."),
            Self::PrivilegeSetup(ERROR_NOT_ALL_ASSIGNED) | Self::NtStatus(0xC0000061) =>
                f.write_str("I/O Priority requires SeIncreaseBasePriorityPrivilege, which Windows did not grant."),
            Self::PrivilegeSetup(code) => write!(f, "Could not enable SeIncreaseBasePriorityPrivilege: Windows error {code}."),
            Self::NtStatus(0xC0000022) => f.write_str("Windows denied access to I/O Priority."),
            Self::NtStatus(status) => write!(f, "NTSTATUS 0x{status:08X}."),
        }
    }
}

fn ensure_privilege() -> Result<(), IoPriorityError> {
    // Winderust and its watchdog each use their own stable primary token.
    static PRIVILEGE: OnceLock<()> = OnceLock::new();
    if PRIVILEGE.get().is_some() {
        return Ok(());
    }
    let mut token = null_mut();
    // SAFETY: the pseudo handle is valid and token is writable; only privilege adjustment is requested.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES, &mut token) } == 0 {
        return Err(IoPriorityError::PrivilegeSetup(last_error()));
    }
    let token = WinHandle::new(token);
    let mut luid = LUID::default();
    // SAFETY: the SDK privilege name is terminated and luid is writable.
    if unsafe { LookupPrivilegeValueW(std::ptr::null(), SE_INC_BASE_PRIORITY_NAME, &mut luid) } == 0
    {
        return Err(IoPriorityError::PrivilegeSetup(last_error()));
    }
    let privileges = TOKEN_PRIVILEGES {
        PrivilegeCount: 1,
        Privileges: [LUID_AND_ATTRIBUTES {
            Luid: luid,
            Attributes: SE_PRIVILEGE_ENABLED,
        }],
    };
    // SAFETY: token has adjustment access and privileges contains exactly one initialized entry.
    let ok =
        unsafe { AdjustTokenPrivileges(token.raw(), 0, &privileges, 0, null_mut(), null_mut()) };
    let error = last_error();
    privilege_adjustment_result(ok != 0, error)?;
    let _ = PRIVILEGE.set(());
    Ok(())
}

fn privilege_adjustment_result(ok: bool, error: u32) -> Result<(), IoPriorityError> {
    if ok && error == 0 {
        Ok(())
    } else {
        Err(IoPriorityError::PrivilegeSetup(error))
    }
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
    with_privilege_retry(|| {
        // SAFETY: process is a verified live handle and priority points to exactly the supplied u32
        // size required by process information class 33.
        unsafe {
            NtSetInformationProcess(
                process.raw(),
                PROCESS_IO_PRIORITY,
                (&mut priority as *mut u32).cast(),
                std::mem::size_of::<u32>() as u32,
            )
        }
    })
}

pub(crate) fn with_privilege_retry(operation: impl FnMut() -> i32) -> Result<(), IoPriorityError> {
    retry_after_privilege(operation, ensure_privilege)
}

fn retry_after_privilege(
    mut operation: impl FnMut() -> i32,
    enable: impl FnOnce() -> Result<(), IoPriorityError>,
) -> Result<(), IoPriorityError> {
    let status = operation();
    if status as u32 == 0xC0000061 {
        enable()?;
        status_result(operation())
    } else {
        status_result(status)
    }
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
    fn retries_only_privilege_failures_and_only_once() {
        for final_status in [0, 0xC0000061u32 as i32] {
            let mut calls = 0;
            let result = retry_after_privilege(
                || {
                    calls += 1;
                    if calls == 1 {
                        0xC0000061u32 as i32
                    } else {
                        final_status
                    }
                },
                || Ok(()),
            );
            assert_eq!(calls, 2);
            assert_eq!(result, status_result(final_status));
        }
        for status in [0, 0xC0000022u32 as i32] {
            assert_eq!(
                retry_after_privilege(|| status, || panic!("unneeded privilege adjustment")),
                status_result(status)
            );
        }
        let mut calls = 0;
        assert_eq!(
            retry_after_privilege(
                || {
                    calls += 1;
                    0xC0000061u32 as i32
                },
                || Err(IoPriorityError::PrivilegeSetup(ERROR_NOT_ALL_ASSIGNED))
            ),
            Err(IoPriorityError::PrivilegeSetup(ERROR_NOT_ALL_ASSIGNED))
        );
        assert_eq!(calls, 1);
    }

    #[test]
    fn privilege_adjustment_requires_assignment_not_just_api_success() {
        assert_eq!(privilege_adjustment_result(true, 0), Ok(()));
        assert_eq!(
            privilege_adjustment_result(true, ERROR_NOT_ALL_ASSIGNED),
            Err(IoPriorityError::PrivilegeSetup(ERROR_NOT_ALL_ASSIGNED))
        );
        assert_eq!(
            privilege_adjustment_result(false, 5),
            Err(IoPriorityError::PrivilegeSetup(5))
        );
    }

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
