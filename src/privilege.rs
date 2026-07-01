use windows_sys::Win32::{
    Foundation::HWND,
    Foundation::{ERROR_NOT_ALL_ASSIGNED, LUID},
    Security::{
        AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_INCREASE_QUOTA_NAME,
        SE_PRIVILEGE_ENABLED, SE_PROF_SINGLE_PROCESS_NAME,
        TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
    UI::{
        Shell::{IsUserAnAdmin, ShellExecuteW},
        WindowsAndMessaging::SW_SHOWNORMAL,
    },
};

use crate::win_util::{last_error, WinHandle};

pub fn relaunch_as_admin_if_needed() -> bool {
    if unsafe { IsUserAnAdmin() } != 0 {
        return false;
    }

    let Ok(exe) = std::env::current_exe() else {
        return true;
    };
    let operation = wide("runas");
    let file = wide(exe.as_os_str());

    unsafe {
        ShellExecuteW(
            std::ptr::null_mut::<std::ffi::c_void>() as HWND,
            operation.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        );
    }

    true
}

pub fn enable_increase_quota_privilege() -> bool {
    enable_privilege(SE_INCREASE_QUOTA_NAME)
}

pub fn enable_profile_single_process_privilege() -> bool {
    enable_privilege(SE_PROF_SINGLE_PROCESS_NAME)
}

fn enable_privilege(name: windows_sys::core::PCWSTR) -> bool {
    let mut token = std::ptr::null_mut();
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

fn wide(value: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;

    value
        .as_ref()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}
