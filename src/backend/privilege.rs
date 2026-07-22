use std::{ffi::OsStr, os::windows::ffi::OsStrExt};

use windows_sys::Win32::{
    Foundation::{ERROR_NOT_ALL_ASSIGNED, LUID},
    Security::{
        AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_INCREASE_QUOTA_NAME,
        SE_PRIVILEGE_ENABLED, SE_PROF_SINGLE_PROCESS_NAME, TOKEN_ADJUST_PRIVILEGES,
        TOKEN_PRIVILEGES, TOKEN_QUERY,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
    UI::{
        Shell::{IsUserAnAdmin, ShellExecuteW},
        WindowsAndMessaging::SW_SHOWNORMAL,
    },
};

use crate::win_util::{last_error, WinHandle};

pub fn enable_increase_quota_privilege() -> bool {
    enable_privilege(SE_INCREASE_QUOTA_NAME)
}

pub fn enable_profile_single_process_privilege() -> bool {
    enable_privilege(SE_PROF_SINGLE_PROCESS_NAME)
}

pub fn is_running_as_admin() -> bool {
    // SAFETY: IsUserAnAdmin takes no arguments and has no caller requirements.
    unsafe { IsUserAnAdmin() != 0 }
}

pub fn relaunch_as_admin() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };

    let operation = wide("runas");
    let file = wide_os(exe.as_os_str());
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

fn enable_privilege(name: windows_sys::core::PCWSTR) -> bool {
    let mut token = std::ptr::null_mut();
    // SAFETY: token is a writable out-pointer; GetCurrentProcess returns the valid pseudo-handle
    // for this process and the requested access mask is documented for token adjustment.
    let opened = unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        )
    };
    if opened == 0 {
        return false;
    }

    let token = WinHandle::new(token);
    enable_privilege_for_token(token.raw(), name)
}

fn enable_privilege_for_token(
    token: windows_sys::Win32::Foundation::HANDLE,
    name: windows_sys::core::PCWSTR,
) -> bool {
    let mut luid = LUID::default();
    // SAFETY: name is a Windows-provided terminated privilege string and luid is writable.
    let found = unsafe { LookupPrivilegeValueW(std::ptr::null(), name, &mut luid) };
    if found == 0 {
        return false;
    }

    let privileges = TOKEN_PRIVILEGES {
        PrivilegeCount: 1,
        Privileges: [LUID_AND_ATTRIBUTES {
            Luid: luid,
            Attributes: SE_PRIVILEGE_ENABLED,
        }],
    };

    // SAFETY: token is a live token handle and privileges points to one fully initialized entry;
    // no previous-state buffer is requested.
    let adjusted = unsafe {
        AdjustTokenPrivileges(
            token,
            0,
            &privileges,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    adjusted != 0 && last_error() != ERROR_NOT_ALL_ASSIGNED
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn wide_os(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}
