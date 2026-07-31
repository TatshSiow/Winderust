use std::{ffi::OsStr, os::windows::ffi::OsStrExt};

use windows_sys::Win32::{
    Foundation::{GetLastError, SetLastError, ERROR_NOT_ALL_ASSIGNED, ERROR_SUCCESS},
    Security::{
        AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED,
        TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
    UI::{
        Shell::{IsUserAnAdmin, ShellExecuteW},
        WindowsAndMessaging::SW_SHOWNORMAL,
    },
};

use crate::win_util::{wide_null, WinHandle};

pub fn is_running_as_admin() -> bool {
    // SAFETY: IsUserAnAdmin takes no arguments and has no caller requirements.
    unsafe { IsUserAnAdmin() != 0 }
}

pub fn enable_debug_privilege() -> Result<(), String> {
    if !is_running_as_admin() {
        return Ok(());
    }

    let mut token = std::ptr::null_mut();
    // SAFETY: GetCurrentProcess returns a process pseudohandle and token points to writable
    // storage for the owned token handle returned by OpenProcessToken.
    if unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        )
    } == 0
    {
        // SAFETY: GetLastError is read immediately after the failed Win32 call.
        let error = unsafe { GetLastError() };
        return Err(format!("OpenProcessToken failed with error {error}."));
    }
    let token = WinHandle::new(token);
    let mut luid = Default::default();
    let privilege_name = wide_null("SeDebugPrivilege");
    // SAFETY: privilege_name is terminated UTF-16 and luid is writable for the call.
    if unsafe { LookupPrivilegeValueW(std::ptr::null(), privilege_name.as_ptr(), &mut luid) } == 0 {
        // SAFETY: GetLastError is read immediately after the failed Win32 call.
        let error = unsafe { GetLastError() };
        return Err(format!("LookupPrivilegeValueW failed with error {error}."));
    }
    let privileges = TOKEN_PRIVILEGES {
        PrivilegeCount: 1,
        Privileges: [LUID_AND_ATTRIBUTES {
            Luid: luid,
            Attributes: SE_PRIVILEGE_ENABLED,
        }],
    };
    // SAFETY: SetLastError only updates this thread's last-error value.
    unsafe { SetLastError(ERROR_SUCCESS) };
    // SAFETY: token is owned with TOKEN_ADJUST_PRIVILEGES, privileges is fully initialized, and
    // no previous-state output is requested.
    let adjusted = unsafe {
        AdjustTokenPrivileges(
            token.raw(),
            0,
            &privileges,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    } != 0;
    // SAFETY: GetLastError is read immediately after AdjustTokenPrivileges because that API can
    // report ERROR_NOT_ALL_ASSIGNED even when its Boolean result is nonzero.
    debug_privilege_adjustment_result(adjusted, unsafe { GetLastError() })
}

fn debug_privilege_adjustment_result(adjusted: bool, error: u32) -> Result<(), String> {
    if !adjusted || error != ERROR_SUCCESS {
        let detail = if error == ERROR_NOT_ALL_ASSIGNED {
            "the elevated token does not contain SeDebugPrivilege".to_owned()
        } else {
            format!("AdjustTokenPrivileges failed with error {error}")
        };
        Err(format!(
            "Could not enable Task Manager-level service access: {detail}."
        ))
    } else {
        Ok(())
    }
}

pub fn relaunch_as_admin() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };

    let operation = wide_null("runas");
    let file = wide_os_null(exe.as_os_str());
    // SAFETY: operation and file are terminated UTF-16 strings, optional parameters are null,
    // and no returned handle is transferred to the caller.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };

    result as isize > 32
}
fn wide_os_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_privilege_rejects_not_all_assigned() {
        assert!(debug_privilege_adjustment_result(true, ERROR_NOT_ALL_ASSIGNED).is_err());
    }
}
